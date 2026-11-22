//! The setup tutorial: real threads in a sandbox, the real thread view (and the real new-thread
//! form) in a frame, a quest panel right of it: per quest a checklist, ✓ per task done, the keys
//! of the next task. The agents are scripted (0 tokens, the same every time) but run the real
//! way: ACP processes under a real host, real edits in a real git repo, `send` between them,
//! flow steps reported with `flow_step`, the real changes tab.
//!
//! Quests: 1 your first thread (send, @mention, watch, changes tab) · 2 many threads (ctrl+p,
//! switch to a seeded thread) · 3 flows (ctrl+p, n: the new-thread form, pick a flow, start,
//! the flow bar runs) · leave (esc esc).
//!
//! - `codehead tutorial <spec json>`: the wizard runs it as a child process. The sandbox (temp
//!   dir: data, config, git repo) lives in its env only: the owner's threads, agents and settings
//!   stay untouched. Exit: 0 done (next screen), 10 back, 130 quit. The sandbox goes at the end.
//! - `codehead tutorial --check <spec json>`: quest 1 without a screen (events printed).
//! - `codehead tutor-agent role=<role> builder=<name> reviewer=<name>`: 1 scripted agent, ACP
//!   over stdin / stdout.

use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::{Terminal, TerminalOptions, Viewport};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::engine::store::{self, Event, Kind};
use crate::theme;

/// An agent of the tutorial: the wizard's crew member.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Member {
    pub name: String,
    pub role: String,
    pub harness: String,
    pub avatar: String,
    pub color: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Spec {
    pub dry: bool,
    pub crew: Vec<Member>,
}

/// Exit codes of `codehead tutorial`.
pub const DONE: i32 = 0;
pub const BACK: i32 = 10;
pub const QUIT: i32 = 130;

/// The widest view: narrower than the split (changes + chat), so the changes tab is a tab.
const VIEW_MAX: u16 = 130;
/// The quest panel: a 3rd of the width, within these; under PANEL_FROM columns: 0 panel, the
/// task on the top row.
const PANEL_MIN: u16 = 40;
const PANEL_MAX: u16 = 60;
const PANEL_FROM: u16 = 100;
/// Under this size: the tutorial asks for a bigger window first.
pub const MIN_W: u16 = 130;
pub const MIN_H: u16 = 34;
/// The 1st prompt, ready in the input.
pub const TASK: &str = "make hello.md say hi to the crew";
/// The seeded 2nd thread.
const SEEDED: &str = "fix the readme";
/// The new thread's prompt, ready in the form.
const FLOW_TASK: &str = "add a line on flows to hello.md";

// ---------- quests ----------

/// 1 task of a quest: done when the owner did it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Task {
    /// The 1st owner message.
    Send,
    /// An owner message to the reviewer (solo: a 2nd owner message).
    Mention,
    /// The reviewer approves after the builder's fix.
    Watch,
    /// The changes tab open.
    Changes,
    /// The menu open (ctrl+p).
    Menu,
    /// The seeded thread open.
    Switch,
    /// The menu open again.
    Menu2,
    /// The new-thread form open (n in the menu).
    New,
    /// A flow picked in the form.
    Flow,
    /// The new thread started.
    Start,
    /// The flow finished.
    FlowRun,
    /// esc esc.
    Leave,
}

pub struct Quest {
    pub title: &'static str,
    pub tasks: Vec<Task>,
}

pub struct Coach {
    pub quests: Vec<Quest>,
    done: Vec<Task>,
    lead: String,
    builder: String,
    reviewer: Option<String>,
    owner_msgs: usize,
    fixed: bool,
}

impl Coach {
    pub fn new(crew: &[Member]) -> Coach {
        let by = |r: &str| crew.iter().find(|m| m.role == r).map(|m| m.name.clone());
        let reviewer = by("reviewer");
        let first = if reviewer.is_some() { vec![Task::Send, Task::Mention, Task::Watch, Task::Changes] } else { vec![Task::Send, Task::Mention, Task::Changes] };
        let quests = vec![
            Quest { title: "your first thread", tasks: first },
            Quest { title: "many threads", tasks: vec![Task::Menu, Task::Switch] },
            Quest { title: "flows", tasks: vec![Task::Menu2, Task::New, Task::Flow, Task::Start, Task::FlowRun] },
            Quest { title: "finish", tasks: vec![Task::Leave] },
        ];
        Coach { quests, done: Vec::new(), lead: crew.first().map(|m| m.name.clone()).unwrap_or_default(), builder: by("builder").unwrap_or_default(), reviewer, owner_msgs: 0, fixed: false }
    }

    fn order(&self) -> Vec<Task> {
        self.quests.iter().flat_map(|q| q.tasks.iter().copied()).collect()
    }

    /// The next task (None: all done).
    pub fn current(&self) -> Option<Task> {
        self.order().into_iter().find(|t| !self.done.contains(t))
    }

    pub fn is_done(&self, t: Task) -> bool {
        self.done.contains(&t)
    }

    /// Only the last task (leave) is left.
    pub fn finished(&self) -> bool {
        matches!(self.current(), Some(Task::Leave) | None)
    }

    /// Task T done, and every task before it (a skipped one never blocks). true: a change.
    pub fn reach(&mut self, t: Task) -> bool {
        if self.done.contains(&t) {
            return false;
        }
        for x in self.order() {
            if !self.done.contains(&x) {
                self.done.push(x);
            }
            if x == t {
                break;
            }
        }
        true
    }

    /// Task T done when it is the next one. true: a change.
    fn tick(&mut self, t: Task) -> bool {
        if self.current() == Some(t) {
            self.done.push(t);
            return true;
        }
        false
    }

    /// What to do, the keys.
    pub fn text(&self, t: Task) -> (String, &'static str) {
        let (lead, b) = (&self.lead, &self.builder);
        match (t, &self.reviewer) {
            (Task::Send, _) => (format!("send the task to {lead}"), "ctrl+enter"),
            (Task::Mention, Some(r)) => (format!("call {r} in: type @{r} review it"), "@ · ctrl+enter"),
            (Task::Mention, None) => (format!("ask again: @{lead} add a 2nd line"), "ctrl+enter"),
            (Task::Watch, Some(r)) => (format!("watch {r} and {b} work it out"), "wait"),
            (Task::Watch, None) => (String::new(), ""),
            (Task::Changes, _) => ("see what changed: the changes tab".into(), "F2"),
            (Task::Menu, _) => ("open the menu".into(), "ctrl+p"),
            (Task::Switch, _) => (format!("open the thread \"{SEEDED}\""), "↑↓ · enter"),
            (Task::Menu2, _) => ("open the menu again".into(), "ctrl+p"),
            (Task::New, _) => ("start a new thread".into(), "n"),
            (Task::Flow, _) => ("pick the flow \"chore\" on an agent card".into(), "f on a card · enter"),
            (Task::Start, _) => ("start the thread".into(), "ctrl+enter"),
            (Task::FlowRun, _) => ("watch the flow bar: 3 steps".into(), "wait"),
            (Task::Leave, _) => ("done. leave the tutorial".into(), "esc esc"),
        }
    }

    /// New events of a thread. true: a change.
    pub fn see(&mut self, events: &[Event]) -> bool {
        let mut moved = false;
        for e in events {
            match &e.kind {
                Kind::Prompt { from, to, text } if from == "owner" => {
                    self.owner_msgs += 1;
                    let to_reviewer = self.reviewer.as_ref().is_some_and(|r| to.contains(r) || text.contains(&format!("@{r}")));
                    moved |= self.tick(Task::Send);
                    if to_reviewer || (self.reviewer.is_none() && self.owner_msgs >= 2) {
                        moved |= self.tick(Task::Mention);
                    }
                }
                Kind::Prompt { from, to, .. } if *from == self.builder && self.reviewer.as_ref().is_some_and(|r| to.contains(r)) => self.fixed = true,
                Kind::TurnEnd { .. } if self.fixed && e.agent.is_some() && e.agent == self.reviewer => moved |= self.tick(Task::Watch),
                Kind::Flow { op, .. } if op == "finished" => moved |= self.tick(Task::FlowRun),
                _ => {}
            }
        }
        moved
    }

    /// The view's state. true: a change.
    pub fn see_view(&mut self, p: &crate::view::Peek) -> bool {
        let mut moved = false;
        if p.pane == crate::view::Pane::Changes {
            moved |= self.tick(Task::Changes);
        }
        if p.menu {
            moved |= self.tick(Task::Menu) || self.tick(Task::Menu2);
        }
        moved
    }
}

// ---------- the sandbox ----------

fn git(dir: &Path, args: &[&str]) -> Result<(), String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=codehead", "-c", "user.email=tutorial@codehead.local", "-c", "commit.gpgsign=false"])
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if out.status.success() { Ok(()) } else { Err(format!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim())) }
}

/// The ACP harness a scripted agent runs as: the member's when it speaks ACP, else opencode.
fn acp_harness(h: &str) -> String {
    match crate::engine::harnesses::get(h) {
        Some(x) if matches!(x.drive, crate::engine::harnesses::Drive::Acp(_)) => h.to_string(),
        _ => "opencode".into(),
    }
}

fn turn_end() -> Kind {
    Kind::TurnEnd { cost: 0.0, input: 0, output: 0, cache_read: 0, cache_write: 0, ctx: 0, window: 0, ms: 0, error: None }
}

/// The 2nd thread: a finished fix of the readme by the builder, in the log as if it ran.
fn seed(repo: &Path, builder: &str) -> Result<String, String> {
    let mut t = store::Thread::new(SEEDED, "fix the typo in README.md", repo, vec![builder.to_string()]);
    t.state = "done".into();
    t.save()?;
    let readme = repo.join("README.md").display().to_string();
    for e in [
        Event::new(None, Kind::Prompt { from: "owner".into(), to: vec![builder.to_string()], text: "fix the typo in README.md".into() }),
        Event::new(Some(builder), Kind::Text { text: "on it.".into() }),
        Event::new(Some(builder), Kind::Tool { id: "s1".into(), name: "Edit".into(), input: json!({"file_path": readme}) }),
        Event::new(Some(builder), Kind::ToolResult { id: "s1".into(), ok: true, output: "ok".into() }),
        Event::new(Some(builder), Kind::Text { text: "fixed: \"teh crew\" → \"the crew\" in README.md.".into() }),
        Event::new(Some(builder), turn_end()),
    ] {
        store::append(&t.id, &e)?;
    }
    Ok(t.id)
}

/// Make the sandbox at ROOT (its env set already): settings with the owner's look, the crew
/// as scripted agents, a git repo with hello.md + README.md, the seeded thread, the thread.
/// (thread, seeded thread, repo).
fn sandbox(root: &Path, look: &crate::config::File, crew: &[Member]) -> Result<(String, String, PathBuf), String> {
    let cfg = crate::config::File {
        scheme: look.scheme.clone(),
        colors: look.colors.clone(),
        messages: look.messages.clone(),
        thoughts: look.thoughts.clone(),
        menu_key: look.menu_key.clone(),
        command_key: look.command_key.clone(),
        agent: crew.first().map(|m| m.name.clone()),
        // the review buttons of the changes view: 0 model calls
        review: Some("off".into()),
        rank: Some("off".into()),
        classify: Some("off".into()),
        ..Default::default()
    };
    crate::config::save_file(&crate::config::path(), "", &cfg)?;
    crate::config::apply(cfg);
    let exe = crate::engine::exe();
    let by = |r: &str| crew.iter().find(|m| m.role == r).map(|m| m.name.clone()).unwrap_or_default();
    for m in crew {
        let avatar = crate::avatar::Avatar::parse(&m.avatar).unwrap_or_default();
        let a = crate::engine::roster::Agent {
            name: m.name.clone(),
            harness: acp_harness(&m.harness),
            mode: "autopilot".into(),
            color: Some(m.color.clone()),
            sprite: Some(crate::engine::roster::SpriteRec { id: avatar.id(), rows: avatar.rows(crate::avatar::Status::Idle, 0.0).to_vec() }),
            command: Some(format!("'{}' tutor-agent role={} builder={} reviewer={}", exe.display(), m.role, by("builder"), by("reviewer"))),
            ..Default::default()
        };
        crate::engine::roster::save(&a)?;
    }
    let repo = root.join("hello");
    std::fs::create_dir_all(&repo).map_err(|e| e.to_string())?;
    std::fs::write(repo.join("hello.md"), "hello\n").map_err(|e| e.to_string())?;
    std::fs::write(repo.join("README.md"), "# hello\n\nteh crew says hi.\n").map_err(|e| e.to_string())?;
    git(&repo, &["init", "-q", "-b", "main"])?;
    git(&repo, &["add", "."])?;
    git(&repo, &["commit", "-q", "-m", "hello"])?;
    // the seeded thread's fix: in the work tree (its changes tab shows it)
    std::fs::write(repo.join("README.md"), "# hello\n\nthe crew says hi.\n").map_err(|e| e.to_string())?;
    let seeded = seed(&repo, &by("builder"))?;
    let names: Vec<String> = crew.iter().map(|m| m.name.clone()).collect();
    let t = store::Thread::new("tutorial", "", &repo, names);
    t.save()?;
    Ok((t.id, seeded, repo))
}

// ---------- the screen ----------

/// The rects in a terminal of W x H: the view (inside its frame), the quest panel (None: too
/// narrow).
fn layout(w: u16, h: u16) -> (Rect, Option<Rect>) {
    let pw = if w >= PANEL_FROM { (w / 3).clamp(PANEL_MIN, PANEL_MAX) } else { 0 };
    let room = w.saturating_sub(4 + 2 + if pw > 0 { pw + 2 } else { 0 });
    let vw = room.min(VIEW_MAX);
    let group = vw + 2 + if pw > 0 { pw + 2 } else { 0 };
    let x0 = w.saturating_sub(group) / 2;
    let view = Rect { x: x0 + 1, y: 4, width: vw, height: h.saturating_sub(7) };
    let side = (pw > 0).then_some(Rect { x: x0 + vw + 4, y: 3, width: pw, height: view.height + 2 });
    (view, side)
}

/// The terminal's size now.
fn size() -> (u16, u16) {
    ratatui::crossterm::terminal::size().unwrap_or((120, 40))
}

/// The whole screen blank, TOP's buffers reset to the size now. Not `Terminal::clear`: it asks
/// the terminal for the cursor (up to 2 s without an answer, keys typed then get lost).
fn wipe(top: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>) {
    let (w, h) = size();
    let _ = top.resize(Rect::new(0, 0, w, h));
}

/// A window under MIN_W x MIN_H: a screen that asks for a bigger one, until it is (the size is
/// read again on each resize). None: go on (big enough, or enter: anyway); Some(code): esc
/// back, ctrl+c quit.
fn size_gate(top: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>) -> Option<i32> {
    use ratatui::crossterm::event::{Event as Ev, KeyCode, KeyEventKind, KeyModifiers};
    loop {
        let (w, h) = size();
        if w >= MIN_W && h >= MIN_H {
            return None;
        }
        let _ = top.draw(|f| {
            let full = f.area();
            f.render_widget(ratatui::widgets::Clear, full);
            let lines = vec![
                Line::from(Span::styled("the tutorial needs room", theme::accent_bold())),
                Line::from(""),
                Line::from(vec![Span::styled("this window: ", theme::muted()), Span::styled(format!("{w} × {h}"), Style::default().fg(theme::pal().warn)), Span::styled(format!(" · best from {MIN_W} × {MIN_H}"), theme::muted())]),
                Line::from(Span::styled("make the terminal bigger, or full screen.", theme::text())),
                Line::from(Span::styled("it goes on by itself once it fits.", theme::muted())),
                Line::from(""),
                Line::from(vec![
                    Span::styled("enter", theme::accent()),
                    Span::styled(" go on anyway  ", theme::muted()),
                    Span::styled("esc", theme::accent()),
                    Span::styled(" back  ", theme::muted()),
                    Span::styled("ctrl+c", theme::accent()),
                    Span::styled(" quit", theme::muted()),
                ]),
            ];
            let bw = lines.iter().map(|l| l.width() as u16).max().unwrap_or(0).min(full.width);
            let bh = (lines.len() as u16).min(full.height);
            let r = Rect { x: full.width.saturating_sub(bw) / 2, y: full.height.saturating_sub(bh) / 2, width: bw, height: bh };
            f.render_widget(Paragraph::new(lines), r);
        });
        // codehead's own reader (the one stdin reader of the process)
        for (ev, _) in crate::read_ready(Duration::from_millis(200)).unwrap_or_default() {
            match ev {
                Ev::Key(k) if k.kind != KeyEventKind::Release => match k.code {
                    KeyCode::Enter => return None,
                    KeyCode::Esc => return Some(BACK),
                    KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => return Some(QUIT),
                    _ => {}
                },
                Ev::Resize(..) => {
                    wipe(top);
                }
                _ => {}
            }
        }
    }
}

/// The quest panel: per quest its title, its tasks (✓ done, › next with its keys, ○ later).
fn panel_lines(coach: &Coach) -> Vec<Line<'static>> {
    let cur = coach.current();
    let mut lines = Vec::new();
    for q in &coach.quests {
        let all = q.tasks.iter().all(|t| coach.is_done(*t));
        let active = cur.is_some_and(|c| q.tasks.contains(&c));
        let (mark, st) = if all { ("✓ ", Style::default().fg(theme::pal().ok)) } else if active { ("◆ ", theme::accent_bold()) } else { ("◇ ", theme::muted()) };
        lines.push(Line::from(vec![Span::styled(mark, st), Span::styled(q.title, if active { theme::bold() } else { theme::muted() })]));
        for t in &q.tasks {
            let (what, keys) = coach.text(*t);
            if what.is_empty() {
                continue;
            }
            if coach.is_done(*t) {
                lines.push(Line::from(vec![Span::styled("  ✓ ", Style::default().fg(theme::pal().ok)), Span::styled(what, theme::muted().add_modifier(Modifier::CROSSED_OUT))]));
            } else if Some(*t) == cur {
                lines.push(Line::from(vec![Span::styled("  › ", theme::accent()), Span::styled(what, theme::text())]));
                lines.push(Line::from(vec![Span::raw("    "), Span::styled(format!(" {keys} "), theme::pill())]));
            } else if active {
                lines.push(Line::from(vec![Span::styled("  ○ ", theme::muted()), Span::styled(what, theme::muted())]));
            }
        }
        lines.push(Line::from(""));
    }
    lines
}

/// The rows around the view: header, the frame, the quest panel (or the task on the top row),
/// the footer.
fn chrome(f: &mut ratatui::Frame, coach: &Coach, dry: bool, view: Rect, side: Option<Rect>) {
    let full = f.area();
    let x = view.x.saturating_sub(1);
    let right = side.map_or(view.right() + 1, |s| s.right());
    let w = (right - x).min(full.width.saturating_sub(x));
    let mut head = vec![Span::styled("codehead", theme::bold()), Span::styled("  try it", theme::accent_bold())];
    if dry {
        head.push(Span::styled("  dry run", theme::muted()));
    }
    // shrunk after the start: a hint, the tutorial goes on
    if full.width < MIN_W || full.height < MIN_H {
        head.push(Span::styled(format!("  · small window {} × {}: best from {MIN_W} × {MIN_H}", full.width, full.height), Style::default().fg(theme::pal().warn)));
    }
    f.render_widget(Paragraph::new(Line::from(head)), Rect { x, y: 1, width: w, height: 1 });
    let frame = Rect { x, y: view.y - 1, width: view.width + 2, height: view.height + 2 };
    f.render_widget(Block::default().borders(Borders::ALL).border_style(theme::line()), frame);
    match side {
        Some(s) => {
            let block = Block::default().borders(Borders::ALL).border_style(theme::accent()).title(Span::styled(" quests ", theme::accent_bold()));
            let inner = block.inner(s);
            f.render_widget(block, s);
            // the whole inner area: a shorter list leaves 0 old text
            f.render_widget(Paragraph::new(panel_lines(coach)).wrap(Wrap { trim: false }), Rect { x: inner.x + 1, width: inner.width.saturating_sub(1), ..inner });
        }
        None => {
            let line = coach.current().map(|t| coach.text(t)).map(|(w, k)| format!("› {w}  [{k}]")).unwrap_or_default();
            f.render_widget(Paragraph::new(Span::styled(format!("{line:<width$}", width = frame.width as usize), theme::accent_bold())), Rect { x, y: 2, width: frame.width, height: 1 });
        }
    }
    let leave = if coach.finished() { "next" } else { "back" };
    let keys = [("esc esc", leave), ("ctrl+enter", "send"), ("ctrl+p", "menu"), ("F1-F4", "views"), ("ctrl+c ctrl+c", "quit")];
    let mut spans = Vec::new();
    for (i, (k, what)) in keys.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(*k, theme::accent()));
        spans.push(Span::styled(format!(" {what}"), theme::muted()));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), Rect { x, y: full.height.saturating_sub(2), width: w, height: 1 });
}

/// The screen: chrome + quests, then the stages (a thread's view, the new-thread form) in the
/// frame. The exit code.
fn show(main: &str, seeded: &str, repo: &Path, spec: &Spec, root: &Path) -> i32 {
    let mut out = io::stdout();
    let mut tty = crate::Tty { enhanced: false };
    if tty.enter(&mut out).is_err() {
        return BACK;
    }
    let Ok(mut top) = Terminal::new(ratatui::backend::CrosstermBackend::new(io::stdout())) else {
        tty.leave(&mut out);
        return BACK;
    };
    wipe(&mut top);
    if let Some(code) = size_gate(&mut top) {
        tty.release(&mut out);
        return code;
    }
    let mut coach = Coach::new(&spec.crew);
    crate::view::save_prompt_draft(main, TASK);
    let mut tails: HashMap<String, store::Tail> = HashMap::new();
    let mut stage = Some(main.to_string());
    let mut prev = main.to_string();
    let dry = spec.dry;
    let code = loop {
        // each stage (and each resize): the layout of the size now, the chrome drawn whole
        let (w, h) = size();
        let (view, side) = layout(w, h);
        wipe(&mut top);
        let _ = top.draw(|f| chrome(f, &coach, dry, view, side));
        let Some(id) = stage.clone() else {
            // the new-thread form
            match new_thread(&mut top, &mut coach, spec, repo, root) {
                Some(Form::Started(new)) => stage = Some(new),
                Some(Form::Quit) => break QUIT,
                _ => stage = Some(prev.clone()),
            }
            continue;
        };
        let Ok(mut term) = Terminal::with_options(ratatui::backend::CrosstermBackend::new(io::stdout()), TerminalOptions { viewport: Viewport::Fixed(view) }) else { break BACK };
        let dir = store::load(&id).map(|t| t.work_dir()).unwrap_or_else(|| repo.to_path_buf());
        let title = store::load(&id).map(|t| t.title).unwrap_or_default();
        let tail = tails.entry(id.clone()).or_insert_with(|| store::Tail::new(&id));
        let res = {
            let mut hook = |p: &crate::view::Peek| {
                let events = tail.read();
                if coach.see(&events) | coach.see_view(p) {
                    let _ = top.draw(|f| chrome(f, &coach, dry, view, side));
                }
                // a resize: the view ends, the loop lays the screen out again
                size() != (w, h)
            };
            crate::view::run_with(&mut term, &id, &dir, &title, None, main, Some(&mut hook))
        };
        match res {
            Ok(None) => {}
            Ok(Some(crate::view::Exit::Open { id: new, .. })) => {
                if new == seeded {
                    coach.reach(Task::Switch);
                }
                prev = id;
                stage = Some(new);
            }
            Ok(Some(crate::view::Exit::Home(crate::menu::Home::New))) => {
                coach.reach(Task::New);
                prev = id;
                stage = None;
            }
            // other start-screen tabs: not in the tutorial, the thread again
            Ok(Some(crate::view::Exit::Home(_))) => {}
            Ok(Some(crate::view::Exit::Back)) => break if coach.finished() { DONE } else { BACK },
            Ok(Some(crate::view::Exit::Quit)) => break QUIT,
            Err(_) => break BACK,
        }
    };
    tty.release(&mut out);
    code
}

/// How the new-thread form ended.
enum Form {
    /// The new thread (its id).
    Started(String),
    /// esc esc, or the form closed.
    Back,
    /// ctrl+c ctrl+c.
    Quit,
}

/// The real new-thread form in the frame, the prompt ready: a flow picked, started → the new
/// thread (with its flow; 0 flow picked: chore). None: the terminal failed.
fn new_thread(top: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>, coach: &mut Coach, spec: &Spec, repo: &Path, root: &Path) -> Option<Form> {
    let notes = root.join("notes");
    let _ = std::fs::create_dir_all(&notes);
    let cfg = crate::app::Config {
        notes,
        repo: Some(repo.to_path_buf()),
        cwd: repo.to_path_buf(),
        agent: spec.crew[0].name.clone(),
        picks: Vec::new(),
        branch: None,
        prefix: "task".into(),
        copy_mode: false,
        logs: crate::engine::logs_dir(),
        drafts: crate::drafts::dir(),
    };
    let mut app = crate::app::App::new(cfg, "");
    app.go_home(crate::menu::Home::New);
    app.set_prompt(FLOW_TASK);
    let fixed = |view: Rect| Terminal::with_options(ratatui::backend::CrosstermBackend::new(io::stdout()), TerminalOptions { viewport: Viewport::Fixed(view) }).ok();
    let mut wh = size();
    let (mut view, mut side) = layout(wh.0, wh.1);
    let mut term = fixed(view)?;
    let (mut last_esc, mut last_ctrl_c) = (None, None);
    loop {
        // a resize: the chrome whole again, a terminal for the new frame (the form keeps its state)
        if size() != wh {
            wh = size();
            (view, side) = layout(wh.0, wh.1);
            wipe(top);
            let _ = top.draw(|f| chrome(f, coach, spec.dry, view, side));
            term = fixed(view)?;
        }
        let _ = term.draw(|f| app.draw(f));
        let (_, flow) = app.flow_peek();
        if flow.is_some() && coach.tick(Task::Flow) {
            let _ = top.draw(|f| chrome(f, coach, spec.dry, view, side));
        }
        if let Some(o) = app.outcome.take() {
            return Some(match o {
                crate::app::Outcome::New { message, agents, flows, .. } => start_thread(coach, repo, &message, agents, flows, &spec.crew[0].name).map_or(Form::Back, Form::Started),
                _ => Form::Back,
            });
        }
        // an editor on a note / the prompt: not in the tutorial
        let _ = app.suspend.take();
        for (ev, _) in crate::read_ready(Duration::from_millis(50)).ok()? {
            match ev {
                ratatui::crossterm::event::Event::Key(k) if k.kind != ratatui::crossterm::event::KeyEventKind::Release => {
                    // esc esc: back to the thread before (the real form goes to the start
                    // screen); ctrl+c ctrl+c: the tutorial ends, as in a thread
                    use ratatui::crossterm::event::{KeyCode, KeyModifiers};
                    let twice = |last: &mut Option<std::time::Instant>| {
                        let hit = last.is_some_and(|t| t.elapsed() < Duration::from_millis(500));
                        *last = Some(std::time::Instant::now());
                        hit
                    };
                    if k.code == KeyCode::Esc && twice(&mut last_esc) {
                        return Some(Form::Back);
                    }
                    if k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL) && twice(&mut last_ctrl_c) {
                        return Some(Form::Quit);
                    }
                    app.key(k)
                }
                ratatui::crossterm::event::Event::Mouse(m) => app.mouse(m),
                ratatui::crossterm::event::Event::Paste(s) => app.paste(&s),
                _ => {}
            }
        }
        app.tick();
    }
}

/// The new thread of the form: in the sandbox repo, each agent's flow (0 picked: chore for the
/// lead), the message to its agents. Its id.
fn start_thread(coach: &mut Coach, repo: &Path, message: &str, agents: Vec<String>, flows: Vec<(String, String, usize)>, lead: &str) -> Option<String> {
    let agents = if agents.is_empty() { vec![lead.to_string()] } else { agents };
    let title: String = message.lines().next().unwrap_or("flows").chars().take(60).collect();
    let mut t = store::Thread::new(&title, message, repo, agents.clone());
    let flows = if flows.is_empty() { t.lead().map(|l| vec![(l.to_string(), "chore".to_string(), 0)]).unwrap_or_default() } else { flows };
    let lib = crate::engine::flows::Library::load(Some(repo));
    for (agent, slug, at) in flows {
        if let Some(fl) = lib.flow(&slug) {
            t.set_flow(&agent, Some(crate::engine::flows::run::FlowState::new(fl, &lib, at, crate::engine::now_ms())));
        }
    }
    t.save().ok()?;
    let _ = crate::engine::client::call(&t.id, &json!({"op": "prompt", "to": agents, "text": message}));
    coach.reach(Task::Start);
    Some(t.id)
}

/// Quest 1 without a screen: the owner's messages sent for the owner, the coach followed to the
/// changes tab. Prints the thread's events and hello.md. 0: the script ran through.
fn headless(id: &str, repo: &Path, spec: &Spec) -> i32 {
    let mut coach = Coach::new(&spec.crew);
    let mut tail = store::Tail::new(id);
    let lead = spec.crew[0].name.clone();
    let reviewer = spec.crew.iter().find(|m| m.role == "reviewer").map(|m| m.name.clone());
    let start = std::time::Instant::now();
    let mut sent: Vec<Task> = Vec::new();
    let mut all = Vec::new();
    while start.elapsed() < Duration::from_secs(60) && coach.current() != Some(Task::Changes) {
        let busy = !crate::engine::client::state(id).1.is_empty();
        let cur = coach.current();
        if let Some(t @ (Task::Send | Task::Mention)) = cur
            && !busy
            && !sent.contains(&t)
        {
            let (to, text) = match (t, &reviewer) {
                (Task::Send, _) => (lead.clone(), TASK.to_string()),
                (_, Some(r)) => (r.clone(), format!("@{r} review it")),
                (_, None) => (lead.clone(), format!("@{lead} add a 2nd line")),
            };
            if let Err(e) = crate::engine::client::call(id, &json!({"op": "prompt", "to": [to], "text": text})) {
                println!("prompt: {e}");
                return 1;
            }
            sent.push(t);
        }
        std::thread::sleep(Duration::from_millis(300));
        let events = tail.read();
        coach.see(&events);
        all.extend(events);
    }
    for e in &all {
        let who = e.agent.clone().unwrap_or_else(|| "-".into());
        match &e.kind {
            Kind::Prompt { from, to, text } => println!("prompt {from} → {}: {text}", to.join(",")),
            Kind::Text { text } => println!("text {who}: {text}"),
            Kind::Tool { name, .. } => println!("tool {who}: {name}"),
            Kind::Error { message } => println!("error {who}: {message}"),
            Kind::TurnEnd { .. } => println!("turn end {who}"),
            _ => {}
        }
    }
    println!("hello.md: {:?}", std::fs::read_to_string(repo.join("hello.md")).unwrap_or_default());
    if coach.current() != Some(Task::Changes) {
        println!("next task: {:?}", coach.current());
        return 1;
    }

    // quest 3's thread: chore, run by the lead's steps
    coach.reach(Task::Flow);
    let Some(fid) = start_thread(&mut coach, repo, FLOW_TASK, Vec::new(), Vec::new(), &lead) else {
        println!("flow thread: 0 start");
        return 1;
    };
    let mut ftail = store::Tail::new(&fid);
    let start_at = std::time::Instant::now();
    while start_at.elapsed() < Duration::from_secs(60) && coach.current() == Some(Task::FlowRun) {
        std::thread::sleep(Duration::from_millis(300));
        for e in ftail.read() {
            match &e.kind {
                Kind::Flow { op, text } => println!("flow {op}: {text}"),
                Kind::Text { text } => println!("text {}: {text}", e.agent.clone().unwrap_or_default()),
                Kind::Error { message } => println!("error: {message}"),
                _ => {}
            }
            coach.see(std::slice::from_ref(&e));
        }
    }
    println!("next task: {:?}", coach.current());
    if coach.finished() { DONE } else { 1 }
}

/// `codehead tutorial [--check] <spec json>`.
pub fn run(args: &[String]) -> ! {
    let check = args.first().is_some_and(|a| a == "--check");
    let arg = args.iter().find(|a| a.starts_with('{')).map(String::as_str);
    let spec: Spec = arg.and_then(|a| serde_json::from_str(a).ok()).unwrap_or_else(|| crate::fail("codehead tutorial: spec json required", 2));
    if spec.crew.is_empty() {
        crate::fail("codehead tutorial: 0 agents", 2);
    }
    // the owner's look before the env moves to the sandbox
    let look = crate::config::read_layers(&crate::config::path()).map(|l| l.merged()).unwrap_or_default();
    sweep();
    let root = sandbox_root(std::process::id());
    let _ = std::fs::remove_dir_all(&root);
    let run = run_dir(std::process::id());
    let _ = std::fs::create_dir_all(root.join("home"));
    // 1 thread at this point: 0 other thread reads the env. HOME too: drafts, seen marks, paste
    // dirs, snapshots (~/.cache/codehead/…) stay in the sandbox, 0 trace in the owner's lists
    unsafe {
        std::env::set_var("HOME", root.join("home"));
        std::env::set_var("CODEHEAD_DRAFTS", root.join("home/drafts"));
        std::env::set_var("CODEHEAD_RUN", &run);
        std::env::set_var("CODEHEAD_HOME", root.join("data"));
        std::env::set_var("XDG_CONFIG_HOME", root.join("config"));
        std::env::set_var("CODEHEAD_CONFIG", root.join("config/codehead.toml"));
        std::env::set_var("CODEHEAD_HOME_REPO", "");
        std::env::set_var("CODEHEAD_AGENT", "");
        std::env::set_var("CODEHEAD_REPO", "");
        std::env::remove_var("CODEHEAD_NOTES");
    }
    let code = match sandbox(&root, &look, &spec.crew) {
        Ok((id, seeded, repo)) => {
            let code = if check { headless(&id, &repo, &spec) } else { show(&id, &seeded, &repo, &spec, &root) };
            for t in store::all() {
                let _ = crate::engine::client::send_within(&t.id, &json!({"op": "stop"}), Duration::from_secs(2));
            }
            std::thread::sleep(Duration::from_millis(300));
            code
        }
        Err(e) => {
            eprintln!("tutorial: {e}");
            BACK
        }
    };
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&run);
    std::process::exit(code)
}

/// The sandbox of the tutorial process PID.
fn sandbox_root(pid: u32) -> PathBuf {
    std::env::temp_dir().join(format!("codehead-tutorial-{pid}"))
}

/// The host sockets of the tutorial process PID: short (a socket path holds max. 104 bytes).
fn run_dir(pid: u32) -> PathBuf {
    PathBuf::from(format!("/tmp/codehead-tutorial-{pid}"))
}

/// Sandboxes of tutorials whose process is gone (a kill, a crash): their hosts stopped, the
/// dirs deleted.
fn sweep() {
    for base in [std::env::temp_dir(), PathBuf::from("/tmp")] {
        let Ok(rd) = std::fs::read_dir(&base) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Some(pid) = name.strip_prefix("codehead-tutorial-").and_then(|p| p.parse::<i32>().ok()) else { continue };
            // signal 0: the process is there (EPERM: there, another user's)
            let alive = unsafe { libc::kill(pid, 0) } == 0 || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM);
            if alive {
                continue;
            }
            // hosts of a gone sandbox: they run on, idle, until stopped
            if let Ok(sockets) = std::fs::read_dir(e.path()) {
                for s in sockets.flatten() {
                    if let Ok(c) = std::os::unix::net::UnixStream::connect(s.path()) {
                        let mut c = c;
                        let _ = writeln!(c, "{}", json!({"op": "stop"}));
                    }
                }
            }
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

// ---------- the scripted agent ----------

struct Agent {
    out: io::Stdout,
    session: String,
    cwd: PathBuf,
    me: String,
    thread: String,
    next: u64,
}

impl Agent {
    fn send(&mut self, v: &Value) {
        let _ = writeln!(self.out, "{v}");
        let _ = self.out.flush();
    }

    fn update(&mut self, u: Value) {
        let v = json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": self.session, "update": u}});
        self.send(&v);
    }

    /// TEXT word by word, as a model streams.
    fn say(&mut self, text: &str) {
        for (i, w) in text.split(' ').enumerate() {
            let chunk = if i == 0 { w.to_string() } else { format!(" {w}") };
            self.update(json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": chunk}}));
            std::thread::sleep(Duration::from_millis(35));
        }
    }

    /// A tool call: shown, WORK done, its output shown. The output.
    fn tool(&mut self, title: &str, kind: &str, input: Value, work: impl FnOnce(&Path) -> String) -> String {
        self.next += 1;
        let id = format!("t{}", self.next);
        self.update(json!({"sessionUpdate": "tool_call", "toolCallId": id, "title": title, "kind": kind, "status": "in_progress", "rawInput": input}));
        std::thread::sleep(Duration::from_millis(400));
        let output = work(&self.cwd);
        self.update(json!({"sessionUpdate": "tool_call_update", "toolCallId": id, "status": "completed", "rawOutput": output}));
        output
    }

    fn edit(&mut self, text: &str) {
        let path = self.cwd.join("hello.md");
        let text = text.to_string();
        self.tool("Edit hello.md", "edit", json!({"file_path": path.display().to_string()}), move |d| {
            let _ = std::fs::write(d.join("hello.md"), text);
            "ok".into()
        });
    }

    fn read(&mut self) {
        let path = self.cwd.join("hello.md");
        self.tool("Read hello.md", "read", json!({"file_path": path.display().to_string()}), |d| std::fs::read_to_string(d.join("hello.md")).unwrap_or_default());
    }

    /// A message to agent TO, through the host (as the MCP `send` tool does).
    fn hand(&mut self, to: &str, text: &str) {
        let (thread, me) = (self.thread.clone(), self.me.clone());
        let (to, text) = (to.to_string(), text.to_string());
        self.tool("send", "other", json!({"to": [to], "message": text}), move |_| match crate::engine::client::send(&thread, &json!({"op": "send", "from": me, "to": [to], "text": text})) {
            Ok(_) => "sent".into(),
            Err(e) => e,
        });
    }

    /// The flow step done (as the MCP `flow_step` tool does). The next step's text, if it is
    /// this agent's.
    fn step_done(&mut self) -> Option<String> {
        let (thread, me) = (self.thread.clone(), self.me.clone());
        let mut next = None;
        let out = self.tool("flow_step", "other", json!({}), |_| match crate::engine::client::send(&thread, &json!({"op": "flow_step", "agent": me})) {
            Ok(r) => r.to_string(),
            Err(e) => e,
        });
        if let Ok(v) = serde_json::from_str::<Value>(&out) {
            next = v["text"].as_str().map(String::from);
        }
        next
    }
}

/// The step name of a flow message: a line `[codehead flow: chore · step 1/3: implement]` →
/// implement. Only at a line's start: the system prompt names the form in its text.
fn step_of(text: &str) -> Option<String> {
    let head = text.lines().find(|l| l.starts_with("[codehead flow:"))?;
    let head = &head[..head.find(']')?];
    Some(head.rsplit(": ").next()?.trim().to_string())
}

/// The steps of a flow, 1 after the other, while they are this agent's.
fn flow_turn(a: &mut Agent, text: &str) {
    let mut text = text.to_string();
    while let Some(step) = step_of(&text) {
        match step.as_str() {
            "implement" => {
                a.edit("hi crew.\nwelcome aboard.\nflows: 1 step at a time.\n");
                a.say("implemented: 1 line on flows.");
            }
            "verify" => {
                a.read();
                a.say("verified: hello.md reads right.");
            }
            "report" => a.say("report: hello.md got 1 line on flows. the flow is done."),
            other => a.say(&format!("{other}: done.")),
        }
        match a.step_done() {
            Some(next) => text = next,
            None => break,
        }
    }
}

/// The turns of the script: ROLE, the prompt TEXT, turns done before.
fn turn(a: &mut Agent, role: &str, kv: &HashMap<String, String>, text: &str, n: u32) {
    let from_agent = text.lines().any(|l| l.starts_with("[from @"));
    let (builder, reviewer) = (kv.get("builder").cloned().unwrap_or_default(), kv.get("reviewer").cloned().unwrap_or_default());
    std::thread::sleep(Duration::from_millis(500));
    if step_of(text).is_some() {
        return flow_turn(a, text);
    }
    match role {
        "planner" => {
            a.say(&format!("plan: 1 change. hello.md greets the crew. {builder} does it."));
            a.hand(&builder, TASK);
        }
        "builder" if from_agent && !reviewer.is_empty() && text.contains(&format!("[from @{reviewer}]")) => {
            a.edit("hi crew.\nwelcome aboard.\n");
            a.say("fixed: a newline at the end, plus a welcome line.");
            a.hand(&reviewer, "fixed. 1 more look?");
        }
        "builder" if n == 0 => {
            a.edit("hi crew.");
            a.say("done: hello.md says hi to the crew.");
        }
        "builder" => {
            a.edit("hi crew.\nwelcome aboard.\n");
            a.say("added a 2nd line: welcome aboard.");
        }
        "reviewer" if from_agent => {
            a.read();
            a.say("approved. 0 more nits.");
        }
        "reviewer" => {
            a.read();
            a.say(&format!("1 nit: hello.md has 0 newline at the end. sent to {builder}."));
            a.hand(&builder, "hello.md: add a newline at the end, please.");
        }
        _ => a.say("ok."),
    }
}

/// `codehead tutor-agent role=… builder=… reviewer=…`.
pub fn agent(args: &[String]) -> ! {
    let kv: HashMap<String, String> = args.iter().filter_map(|a| a.split_once('=')).map(|(k, v)| (k.to_string(), v.to_string())).collect();
    let role = kv.get("role").cloned().unwrap_or_default();
    let me = std::env::var("CODEHEAD_AGENT").unwrap_or_default();
    let mut a = Agent { out: io::stdout(), session: format!("tutor-{me}"), cwd: PathBuf::from("."), me: me.clone(), thread: std::env::var("CODEHEAD_THREAD").unwrap_or_default(), next: 0 };
    let mut turns = 0u32;
    for line in io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        let id = v.get("id").cloned();
        let reply = |a: &mut Agent, r: Value| {
            if let Some(id) = &id {
                a.send(&json!({"jsonrpc": "2.0", "id": id, "result": r}));
            }
        };
        match v["method"].as_str() {
            Some("initialize") => reply(&mut a, json!({"protocolVersion": 1, "agentCapabilities": {"loadSession": false}})),
            Some("session/new") => {
                if let Some(c) = v["params"]["cwd"].as_str() {
                    a.cwd = PathBuf::from(c);
                }
                let sid = a.session.clone();
                reply(&mut a, json!({"sessionId": sid}));
            }
            Some("session/prompt") => {
                let text: String = v["params"]["prompt"].as_array().map(|p| p.iter().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n")).unwrap_or_default();
                turn(&mut a, &role, &kv, &text, turns);
                turns += 1;
                reply(&mut a, json!({"stopReason": "end_turn"}));
            }
            Some(_) if id.is_some() => reply(&mut a, json!({})),
            _ => {}
        }
    }
    std::process::exit(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crew(roles: &[&str]) -> Vec<Member> {
        roles.iter().zip(["ADA", "NOVA", "ZED"]).map(|(r, n)| Member { name: n.into(), role: r.to_string(), harness: "claude".into(), avatar: "regi:1".into(), color: "blue".into() }).collect()
    }

    fn ev(agent: Option<&str>, kind: Kind) -> Event {
        Event { ts: 0, agent: agent.map(String::from), kind }
    }

    fn prompt(from: &str, to: &[&str], text: &str) -> Kind {
        Kind::Prompt { from: from.into(), to: to.iter().map(|s| s.to_string()).collect(), text: text.into() }
    }

    fn peek(pane: crate::view::Pane, menu: bool) -> crate::view::Peek {
        crate::view::Peek { pane, menu }
    }

    #[test]
    fn coach_follows_a_pair_through_every_quest() {
        let mut c = Coach::new(&crew(&["builder", "reviewer"]));
        assert_eq!(c.current(), Some(Task::Send));
        assert!(c.see(&[ev(None, prompt("owner", &["ADA"], TASK))]));
        assert!(c.text(c.current().unwrap()).0.contains("@NOVA"));

        // a message to the builder again: 0 step
        assert!(!c.see(&[ev(None, prompt("owner", &["ADA"], "more"))]));
        assert!(c.see(&[ev(None, prompt("owner", &["NOVA"], "@NOVA review it"))]));

        // the reviewer's 1st turn end: not yet (the builder has not fixed)
        assert!(!c.see(&[ev(Some("NOVA"), prompt("NOVA", &["ADA"], "nit")), ev(Some("NOVA"), turn_end())]));
        assert!(c.see(&[ev(Some("ADA"), prompt("ADA", &["NOVA"], "fixed")), ev(Some("NOVA"), turn_end())]));

        // the menu before the changes tab: 0 step
        assert!(!c.see_view(&peek(crate::view::Pane::Chat, true)));
        assert!(c.see_view(&peek(crate::view::Pane::Changes, false)));
        assert!(c.see_view(&peek(crate::view::Pane::Changes, true)), "ctrl+p");
        assert_eq!(c.current(), Some(Task::Switch));

        // quest 2 done by the switch; quest 3: the menu, n, a flow, the start, the flow's end
        assert!(c.reach(Task::Switch));
        assert!(c.see_view(&peek(crate::view::Pane::Chat, true)));
        assert!(c.reach(Task::New));
        assert!(c.tick(Task::Flow));
        assert!(c.reach(Task::Start));
        assert!(!c.finished());
        assert!(c.see(&[ev(None, Kind::Flow { op: "finished".into(), text: String::new() })]));
        assert!(c.finished() && c.current() == Some(Task::Leave));
    }

    #[test]
    fn reach_marks_skipped_tasks_and_solo_has_0_watch() {
        let mut c = Coach::new(&crew(&["builder"]));
        assert!(!c.order().contains(&Task::Watch));
        c.see(&[ev(None, prompt("owner", &["ADA"], TASK))]);
        assert!(c.text(Task::Mention).0.contains("@ADA"));
        assert!(c.see(&[ev(None, prompt("owner", &["ADA"], "@ADA add a 2nd line"))]));

        // n in the menu before ctrl+p was ticked: the tasks before count as done
        assert!(c.reach(Task::New));
        assert!(c.is_done(Task::Changes) && c.is_done(Task::Menu) && c.is_done(Task::Switch) && c.is_done(Task::Menu2));
        assert_eq!(c.current(), Some(Task::Flow));
        assert!(!c.reach(Task::New), "again: 0 change");
    }

    #[test]
    fn panel_shows_checks_the_next_keys_and_hides_later_quests() {
        let mut c = Coach::new(&crew(&["builder", "reviewer"]));
        c.reach(Task::Send);
        let text: Vec<String> = panel_lines(&c).iter().map(|l| l.spans.iter().map(|s| s.content.to_string()).collect()).collect();
        assert!(text.iter().any(|l| l.starts_with("  ✓ send the task")), "{text:?}");
        assert!(text.iter().any(|l| l.starts_with("  › call NOVA in")));
        assert!(text.iter().any(|l| l.contains(" @ · ctrl+enter ")));
        assert!(text.iter().any(|l| l.contains("many threads")) && !text.iter().any(|l| l.contains("open the menu")), "later quests: title only");
    }

    #[test]
    fn steps_of_flow_messages() {
        assert_eq!(step_of("[the owner] go\n\n[codehead flow: chore · step 2/3: verify]\n\nverify it").as_deref(), Some("verify"));
        assert_eq!(step_of("[from @NOVA] fixed"), None);
        assert_eq!(step_of("- a message that starts with `[codehead flow: ...]` is a step"), None, "the system prompt");
    }

    #[test]
    fn layout_puts_the_panel_right_and_keeps_the_view_under_the_split() {
        let (v, p) = layout(300, 60);
        let p = p.unwrap();
        assert!(v.width <= VIEW_MAX && p.x > v.right() && p.right() <= 300 && p.width == PANEL_MAX);
        // a 3rd of the width for the quests
        let (_, p) = layout(150, 40);
        assert_eq!(p.unwrap().width, 50);
        let (v, p) = layout(90, 30);
        assert!(p.is_none() && v.width == 84);
    }
}
