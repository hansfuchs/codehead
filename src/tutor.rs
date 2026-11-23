//! The setup tutorial: real threads in a sandbox, the real thread view (and the real new-thread
//! form) in a frame, a quest panel right of it: per quest a checklist, ✓ per task done, the keys
//! of the next task. The agents are scripted (0 tokens, the same every time) but run the real
//! way: ACP processes under a real host, real edits in a real git repo, `send` between them,
//! flow steps reported with `flow_step`, the real changes tab.
//!
//! Quests, in the order of 1 real task: basics: 1 start a task (send it with a style guide
//! attached, the attachment in the files view) · 2 work with the crew (@mention + #shortcut,
//! watch the review) · 3 check and commit (changes view, a line in the IDE (mocked), stage
//! hello.md only, commit). Then the owner may stop (esc esc: done). Advanced: 4 side questions
//! (`>sub`) · 5 ship it (the lead posts a mock PR, merge view, merge) · 6 many threads (ctrl+p,
//! the seeded thread) · 7 new thread with a flow (home repo, notes, the flow bar) · 8 finish.
//! The quest panel shows the current quest only: its concept (what, when), tasks, docs link.
//!
//! Mocks ($CODEHEAD_TUTORIAL, see [`active`]): 0 IDE opens (a toast), 0 browser opens, 0 GitHub
//! call (gh fetch / diff / run / labels answer with the mock PR of the sandbox repo).
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
/// The 1st prompt, ready in the input: the style guide attached as a paste.
pub const TASK: &str = "make hello.md say hi to the crew. style: [Pasted text #1 +3 lines]";
/// The attachment's marker in TASK.
const MARKER: &str = "[Pasted text #1 +3 lines]";
/// The attachment: a style guide (the reviewer's nit follows its last rule).
const STYLE: &str = "- lower case\n- 1 line per idea\n- a newline at the end\n";
/// The mock PR's link, as the lead posts it.
const PR_URL: &str = "https://github.com/crew/hello/pull/7";
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
    /// The files view open (a click on the message's attachment).
    Attach,
    /// An owner message to the reviewer (solo: a 2nd owner message), a #shortcut in it.
    Mention,
    /// The reviewer approves after the builder's fix.
    Watch,
    /// The changes tab open.
    Changes,
    /// An IDE open asked from the changes view (mocked: a toast).
    Ide,
    /// hello.md staged.
    Stage,
    /// A commit of the owner.
    Commit,
    /// A sub-thread started (`>sub`).
    Sub,
    /// The sub-thread's result back.
    SubDone,
    /// An agent posted the PR link.
    AskPr,
    /// The merge view open.
    MergeView,
    /// The mock PR merged.
    Merged,
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
    /// The concept: what it is, when to use it (short lines of the docs).
    pub about: &'static [&'static str],
    /// Its docs page (under DOCS).
    pub doc: &'static str,
    /// After the basics: the owner can stop before it.
    pub advanced: bool,
}

/// The docs root of the quests' links.
const DOCS: &str = "codehead.sh/docs/";

pub struct Coach {
    pub quests: Vec<Quest>,
    done: Vec<Task>,
    lead: String,
    builder: String,
    reviewer: Option<String>,
    owner_msgs: usize,
    fixed: bool,
    /// The last read of the repo (staged, commits, the merge marker): max. 1 per REPO_EVERY.
    repo_at: Option<std::time::Instant>,
}

/// How often the coach reads the repo's git state.
const REPO_EVERY: Duration = Duration::from_millis(700);

impl Coach {
    pub fn new(crew: &[Member]) -> Coach {
        let by = |r: &str| crew.iter().find(|m| m.role == r).map(|m| m.name.clone());
        let reviewer = by("reviewer");
        let crew_tasks = if reviewer.is_some() { vec![Task::Mention, Task::Watch] } else { vec![Task::Mention] };
        let quests = vec![
            Quest {
                title: "start a task",
                tasks: vec![Task::Send, Task::Attach],
                about: &[
                    "1 thread = 1 task: its messages, tool runs and changes.",
                    "the agents run in the background: close codehead, they go on.",
                    "paste or drop a file into a message: your agents read it.",
                ],
                doc: "concepts/threads/",
                advanced: false,
            },
            Quest {
                title: "work with the crew",
                tasks: crew_tasks,
                about: &[
                    "@name: a message to 1 agent. agents talk to each other with send.",
                    "#name: a shortcut, a text you wrote once (settings → shortcuts).",
                    "use shortcuts for what you type again and again: review, plan, ship.",
                ],
                doc: "concepts/messaging/",
                advanced: false,
            },
            Quest {
                title: "check and commit",
                tasks: vec![Task::Changes, Task::Ide, Task::Stage, Task::Commit],
                about: &[
                    "4 views per thread: chat F1 · changes F2 · merge F3 · files F4.",
                    "changes: the diff of the checkout. comment, stage, commit, push.",
                    "e: the line under the cursor in your IDE.",
                    "threads in 1 checkout share it: commit only your task's files.",
                ],
                doc: "concepts/thread-view/",
                advanced: false,
            },
            Quest {
                title: "side questions",
                tasks: vec![Task::Sub, Task::SubDone],
                about: &[
                    ">sub @name task: a sub-thread does it in the same checkout.",
                    "this thread waits, then gets the result: 0 noise in this chat.",
                    "use it for a detour or a check. a new task: a new thread.",
                ],
                doc: "concepts/threads/",
                advanced: true,
            },
            Quest {
                title: "ship it",
                tasks: vec![Task::AskPr, Task::MergeView, Task::Merged],
                about: &[
                    "merge F3: the thread's pull requests (its branch, links in its log).",
                    "checks, reviews, comments, labels and the merge: 0 browser.",
                    "here: a mock PR, 0 GitHub calls.",
                ],
                doc: "reference/keys/#merge-view",
                advanced: true,
            },
            Quest {
                title: "many threads",
                tasks: vec![Task::Menu, Task::Switch],
                about: &[
                    "1 thread per task, as many as you like, all at work at once.",
                    "ctrl+p: the menu. every thread and every action, fuzzy search.",
                    "a thread that needs you shows it in the list.",
                ],
                doc: "concepts/threads/",
                advanced: true,
            },
            Quest {
                title: "new thread, with a flow",
                tasks: vec![Task::Menu2, Task::New, Task::Flow, Task::Start, Task::FlowRun],
                about: &[
                    "a flow: the steps an agent takes for a kind of task, 1 at a time.",
                    "gates wait for your ok. loops go back until a condition holds.",
                    "use one for repeat work: feature, bug fix, review. skip it for a quick question.",
                    "home repo: 1 git repo for crew, notes, settings and repos, on every device.",
                    "notes: the new tab attaches them. ask your agents to write them: 0 fixed format.",
                ],
                doc: "concepts/flows/",
                advanced: true,
            },
            Quest {
                title: "finish",
                tasks: vec![Task::Leave],
                about: &["keys to keep: ctrl+p menu · F1-F4 views · @ agents · # shortcuts · >sub side task · esc esc back."],
                doc: "reference/keys/",
                advanced: true,
            },
        ];
        Coach { quests, done: Vec::new(), lead: crew.first().map(|m| m.name.clone()).unwrap_or_default(), builder: by("builder").unwrap_or_default(), reviewer, owner_msgs: 0, fixed: false, repo_at: None }
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

    /// The quest of the next task (None: all done).
    pub fn quest(&self) -> Option<usize> {
        let cur = self.current()?;
        self.quests.iter().position(|q| q.tasks.contains(&cur))
    }

    /// Every task of the basics done.
    pub fn basics_done(&self) -> bool {
        self.quests.iter().filter(|q| !q.advanced).flat_map(|q| &q.tasks).all(|t| self.done.contains(t))
    }

    /// The owner may leave as done: the basics done (the advanced quests are optional).
    pub fn finished(&self) -> bool {
        self.basics_done()
    }

    /// The basics done, 0 advanced task yet: the stop offer shows.
    pub fn at_stop(&self) -> bool {
        self.basics_done() && !self.quests.iter().filter(|q| q.advanced).flat_map(|q| &q.tasks).any(|t| self.done.contains(t))
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
        let side = self.reviewer.as_deref().unwrap_or(lead);
        match (t, &self.reviewer) {
            (Task::Send, _) => (format!("send the task to {lead}. it holds a style guide"), "ctrl+enter"),
            (Task::Attach, _) => (format!("click {MARKER} in your message: the files view shows it"), "click"),
            (Task::Mention, Some(r)) => (format!("back to the chat. call {r} in with a shortcut: @{r} #review"), "F1 · @ · # · ctrl+enter"),
            (Task::Mention, None) => (format!("back to the chat. ask for more with a shortcut: @{lead} #polish"), "F1 · # · ctrl+enter"),
            (Task::Watch, Some(r)) => (format!("watch {r} and {b} work it out"), "wait"),
            (Task::Watch, None) => (String::new(), ""),
            (Task::Changes, _) => ("see what changed: the changes view".into(), "F2"),
            (Task::Ide, _) => ("open a line of hello.md in your IDE (here: a toast only)".into(), "j k · e"),
            (Task::Stage, _) => (format!("stage hello.md only: README.md is the work of \"{SEEDED}\""), "← · j k · space · s"),
            (Task::Commit, _) => ("write a commit message, commit".into(), "m · type · ctrl+enter"),
            (Task::Sub, _) => (format!("a side check before the PR: >sub @{side} does README.md need a change?"), "F1 · >sub · ctrl+enter"),
            (Task::SubDone, _) => ("wait for the sub-thread's result in the chat".into(), "wait"),
            (Task::AskPr, _) => (format!("ask for a pull request: @{lead} open a PR"), "ctrl+enter"),
            (Task::MergeView, _) => ("see the PR in the merge view".into(), "F3"),
            (Task::Merged, _) => ("merge it: the merge stop, enter enter".into(), "j k · enter enter"),
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
                        // the files view skipped: the mention counts all the same
                        if self.current() == Some(Task::Attach) {
                            moved |= self.reach(Task::Attach);
                        }
                        moved |= self.tick(Task::Mention);
                    }
                }
                Kind::Prompt { from, to, .. } if *from == self.builder && self.reviewer.as_ref().is_some_and(|r| to.contains(r)) => self.fixed = true,
                Kind::TurnEnd { .. } if self.fixed && e.agent.is_some() && e.agent == self.reviewer => moved |= self.tick(Task::Watch),
                Kind::SubThread { state, .. } if state == "running" => moved |= self.reach_from(Task::Sub, Task::Commit),
                Kind::SubThread { state, .. } if state == "done" || state == "failed" => moved |= self.tick(Task::SubDone),
                Kind::Text { text } if e.agent.is_some() && text.contains("/pull/") => moved |= self.reach_from(Task::AskPr, Task::Commit),
                Kind::Flow { op, .. } if op == "finished" => moved |= self.tick(Task::FlowRun),
                _ => {}
            }
        }
        moved
    }

    /// T done (+ the tasks before it) once AFTER is done: an advanced task done out of order.
    fn reach_from(&mut self, t: Task, after: Task) -> bool {
        if self.is_done(after) { self.reach(t) } else { false }
    }

    /// The view's state. true: a change.
    pub fn see_view(&mut self, p: &crate::view::Peek) -> bool {
        let mut moved = false;
        match p.pane {
            crate::view::Pane::Files => moved |= self.tick(Task::Attach),
            crate::view::Pane::Changes => moved |= self.tick(Task::Changes),
            crate::view::Pane::Merge => moved |= self.tick(Task::MergeView),
            crate::view::Pane::Chat => {}
        }
        if p.ide {
            moved |= self.tick(Task::Ide);
        }
        if p.menu {
            moved |= self.tick(Task::Menu) || self.tick(Task::Menu2);
        }
        moved
    }

    /// The repo's git state, while a task waits on it (max. 1 read per REPO_EVERY). true: a
    /// change.
    pub fn see_repo(&mut self, repo: &Path) -> bool {
        let Some(t) = self.current().filter(|t| matches!(t, Task::Ide | Task::Stage | Task::Commit | Task::Merged)) else { return false };
        if self.repo_at.is_some_and(|at| at.elapsed() < REPO_EVERY) {
            return false;
        }
        self.repo_at = Some(std::time::Instant::now());
        let out = |args: &[&str]| std::process::Command::new("git").arg("-C").arg(repo).args(args).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
        if t == Task::Merged {
            return mock_merged(repo) && self.tick(Task::Merged);
        }
        // a commit: every task of the quest before it counts as done
        if out(&["rev-list", "--count", "HEAD"]).trim().parse::<u32>().unwrap_or(0) > 1 {
            return self.reach(Task::Commit);
        }
        if t == Task::Stage && out(&["diff", "--cached", "--name-only"]).lines().any(|l| l.trim() == "hello.md") {
            return self.tick(Task::Stage);
        }
        false
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
/// as scripted agents, 2 shortcuts, a git repo with hello.md + README.md, a home repo (AGENTS.md
/// that lists the repo, notes), the style guide to attach, the seeded thread, the thread.
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
    // git in the sandbox (HOME: the sandbox's): an identity, 0 signing. The changes view's
    // commit runs plain git: 0 identity → "Author identity unknown", 0 commit
    std::fs::write(root.join("home/.gitconfig"), "[user]\n\tname = you\n\temail = you@tutorial.codehead.local\n[commit]\n\tgpgsign = false\n[tag]\n\tgpgsign = false\n").map_err(|e| e.to_string())?;
    crate::shortcuts::save(&[
        crate::shortcuts::Shortcut { name: "review".into(), text: "review the change in hello.md against the style guide. send each nit to the agent who wrote it.".into() },
        crate::shortcuts::Shortcut { name: "polish".into(), text: "add a 2nd line: welcome aboard.".into() },
    ])?;
    let repo = root.join("hello");
    std::fs::create_dir_all(&repo).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(root.join("paste")).map_err(|e| e.to_string())?;
    std::fs::write(style_path(root), STYLE).map_err(|e| e.to_string())?;
    // the home repo: the new tab's repo list and notes come from it
    let home = root.join("home-repo");
    std::fs::create_dir_all(home.join("notes")).map_err(|e| e.to_string())?;
    std::fs::write(home.join("AGENTS.md"), format!("# home\n\nsteering for every agent: read me first.\n\n## Other repos\n\n- hello: the tutorial's repo. `{}`\n", repo.display())).map_err(|e| e.to_string())?;
    std::fs::write(home.join("notes/greetings.md"), "# greetings\n\nhow the crew greets: lower case, short.\n").map_err(|e| e.to_string())?;
    git(&home, &["init", "-q", "-b", "main"])?;
    git(&home, &["add", "."])?;
    git(&home, &["commit", "-q", "-m", "home"])?;
    std::fs::write(repo.join("hello.md"), "hello\n").map_err(|e| e.to_string())?;
    std::fs::write(repo.join("README.md"), "# hello\n\nteh crew says hi.\n").map_err(|e| e.to_string())?;
    git(&repo, &["init", "-q", "-b", "main"])?;
    git(&repo, &["add", "."])?;
    git(&repo, &["commit", "-q", "-m", "hello"])?;
    // the mock PR's reviewer
    let _ = std::fs::write(repo.join(".git/codehead-tutorial-reviewer"), by("reviewer"));
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
fn wipe(top: &mut Terminal<ratatui::backend::CrosstermBackend<crate::frames::Frames>>) {
    let (w, h) = size();
    let _ = top.resize(Rect::new(0, 0, w, h));
}

/// A window under MIN_W x MIN_H: a screen that asks for a bigger one, until it is (the size is
/// read again on each resize). None: go on (big enough, or enter: anyway); Some(code): esc
/// back, ctrl+c quit.
fn size_gate(top: &mut Terminal<ratatui::backend::CrosstermBackend<crate::frames::Frames>>) -> Option<i32> {
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
                Line::from(vec![Span::styled("this window: ", theme::muted()), Span::styled(format!("{w} × {h}"), theme::fg(theme::pal().caution())), Span::styled(format!(" · best from {MIN_W} × {MIN_H}"), theme::muted())]),
                Line::from(Span::styled("make the terminal bigger, or full screen.", theme::text())),
                Line::from(Span::styled("it goes on by itself once it fits.", theme::muted())),
                Line::from(""),
                Line::from(vec![
                    Span::styled("enter", theme::bold()),
                    Span::styled(" go on anyway  ", theme::muted()),
                    Span::styled("esc", theme::bold()),
                    Span::styled(" back  ", theme::muted()),
                    Span::styled("ctrl+c", theme::bold()),
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

/// The quest panel: the page of the current quest only. A row of dots (1 per quest), its title,
/// its concept (what, when), its tasks (✓ done, › next with its keys, ○ later), its docs link.
/// The basics done, 0 advanced task yet: the stop offer on top.
fn panel_lines(coach: &Coach, width: u16) -> Vec<Line<'static>> {
    let width = width as usize;
    let cur = coach.current();
    let at = coach.quest().unwrap_or(coach.quests.len().saturating_sub(1));
    let ok = Style::default().fg(theme::pal().success());
    let mut lines = Vec::new();
    // the dots: ✓ done quests, ◆ this one, ○ later; a gap between the basics and the advanced
    let mut dots = Vec::new();
    for (i, q) in coach.quests.iter().enumerate() {
        if i > 0 && q.advanced && !coach.quests[i - 1].advanced {
            dots.push(Span::styled(" │", theme::muted()));
        }
        let all = q.tasks.iter().all(|t| coach.is_done(*t));
        dots.push(match (all, i == at) {
            (true, _) => Span::styled(" ✓", ok),
            (false, true) => Span::styled(" ◆", theme::accent_bold()),
            _ => Span::styled(" ○", theme::muted()),
        });
    }
    lines.push(Line::from(dots));
    lines.push(Line::from(""));
    if coach.at_stop() {
        lines.extend(wrap(Span::styled("✓ ", ok), "basics done: you know enough to work.", theme::bold(), width));
        lines.extend(wrap(Span::raw("  "), "esc esc: stop here, explore the rest on your own.", theme::text(), width));
        lines.extend(wrap(Span::raw("  "), "or go on: the advanced quests.", theme::muted(), width));
        lines.push(Line::from(""));
    }
    let Some(q) = coach.quests.get(at) else { return lines };
    let kind = if q.advanced { "advanced" } else { "basics" };
    lines.push(Line::from(vec![
        Span::styled(format!("{}/{} ", at + 1, coach.quests.len()), theme::accent_bold()),
        Span::styled(q.title, theme::bold()),
        Span::styled(format!("  {kind}"), theme::muted()),
    ]));
    lines.push(Line::from(""));
    for a in q.about {
        lines.extend(wrap(Span::styled("│ ", theme::line()), a, theme::text(), width));
    }
    lines.push(Line::from(""));
    for t in &q.tasks {
        let (what, keys) = coach.text(*t);
        if what.is_empty() {
            continue;
        }
        if coach.is_done(*t) {
            lines.extend(wrap(Span::styled("  ✓ ", ok), &what, theme::muted().add_modifier(Modifier::CROSSED_OUT), width));
        } else if Some(*t) == cur {
            lines.extend(wrap(Span::styled("  › ", theme::accent()), &what, theme::text(), width));
            lines.push(Line::from(vec![Span::raw("    "), Span::styled(format!(" {keys} "), theme::pill())]));
        } else {
            lines.extend(wrap(Span::styled("  ○ ", theme::muted()), &what, theme::muted(), width));
        }
    }
    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled("more: ", theme::muted()), Span::styled(format!("{DOCS}{}", q.doc), theme::accent())]));
    lines
}

/// TEXT in STYLE wrapped at word ends to WIDTH columns: HEAD before the 1st row, blanks of its
/// width before the rest (a hanging indent; a `│` head repeats).
fn wrap(head: Span<'static>, text: &str, style: Style, width: usize) -> Vec<Line<'static>> {
    use unicode_width::UnicodeWidthStr;
    let hw = head.content.width();
    let rest = if head.content.trim() == "│" { head.clone() } else { Span::raw(" ".repeat(hw)) };
    let room = width.saturating_sub(hw).max(8);
    let mut rows: Vec<String> = vec![String::new()];
    for w in text.split(' ') {
        let cur = rows.last_mut().expect("1 row");
        if !cur.is_empty() && cur.width() + 1 + w.width() > room {
            rows.push(w.to_string());
        } else {
            if !cur.is_empty() {
                cur.push(' ');
            }
            cur.push_str(w);
        }
    }
    rows.into_iter().enumerate().map(|(i, r)| Line::from(vec![if i == 0 { head.clone() } else { rest.clone() }, Span::styled(r, style)])).collect()
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
        head.push(Span::styled(format!("  · small window {} × {}: best from {MIN_W} × {MIN_H}", full.width, full.height), theme::fg(theme::pal().caution())));
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
            let width = inner.width.saturating_sub(1);
            f.render_widget(Paragraph::new(panel_lines(coach, width)).wrap(Wrap { trim: false }), Rect { x: inner.x + 1, width, ..inner });
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
        spans.push(Span::styled(*k, theme::bold()));
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
    let Ok(mut top) = Terminal::new(crate::frames::backend()) else {
        tty.leave(&mut out);
        return BACK;
    };
    wipe(&mut top);
    if let Some(code) = size_gate(&mut top) {
        tty.release(&mut out);
        return code;
    }
    let mut coach = Coach::new(&spec.crew);
    crate::view::save_prompt_draft_files(main, TASK, &[(MARKER.to_string(), style_path(root))]);
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
        let Ok(mut term) = Terminal::with_options(crate::frames::backend(), TerminalOptions { viewport: Viewport::Fixed(view) }) else { break BACK };
        let dir = store::load(&id).map(|t| t.work_dir()).unwrap_or_else(|| repo.to_path_buf());
        let title = store::load(&id).map(|t| t.title).unwrap_or_default();
        let tail = tails.entry(id.clone()).or_insert_with(|| store::Tail::new(&id));
        let res = {
            let mut hook = |p: &crate::view::Peek| {
                let events = tail.read();
                if coach.see(&events) | coach.see_view(p) | coach.see_repo(repo) {
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
fn new_thread(top: &mut Terminal<ratatui::backend::CrosstermBackend<crate::frames::Frames>>, coach: &mut Coach, spec: &Spec, repo: &Path, root: &Path) -> Option<Form> {
    let notes = root.join("home-repo/notes");
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
    let fixed = |view: Rect| Terminal::with_options(crate::frames::backend(), TerminalOptions { viewport: Viewport::Fixed(view) }).ok();
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

/// The quests without a screen: the owner's messages sent for the owner (shortcuts expanded,
/// the attachment listed), the view tasks counted as done, the commit made with git, the mock
/// PR merged through the gh mock, the flow thread run. Prints the threads' events and hello.md.
/// 0: the script ran through.
fn headless(id: &str, repo: &Path, spec: &Spec, root: &Path) -> i32 {
    let mut coach = Coach::new(&spec.crew);
    let mut tail = store::Tail::new(id);
    let lead = spec.crew[0].name.clone();
    let reviewer = spec.crew.iter().find(|m| m.role == "reviewer").map(|m| m.name.clone());
    let shortcut = |name: &str| crate::shortcuts::expand(&format!("#{name}"));
    let print = |e: &Event| {
        let who = e.agent.clone().unwrap_or_else(|| "-".into());
        match &e.kind {
            Kind::Prompt { from, to, text } => println!("prompt {from} → {}: {}", to.join(","), text.lines().next().unwrap_or_default()),
            Kind::Text { text } => println!("text {who}: {text}"),
            Kind::Tool { name, .. } => println!("tool {who}: {name}"),
            Kind::Error { message } => println!("error {who}: {message}"),
            Kind::TurnEnd { .. } => println!("turn end {who}"),
            Kind::Flow { op, text } => println!("flow {op}: {text}"),
            _ => {}
        }
    };
    // the owner's message TEXT to TO, once the thread is idle
    let prompt = |to: &str, text: String| -> Result<(), String> { crate::engine::client::call(id, &json!({"op": "prompt", "to": [to], "text": text})).map(|_| ()) };
    let mut sent: Vec<Task> = Vec::new();
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(120) && coach.current().is_some_and(|t| !matches!(t, Task::Changes)) {
        let busy = !crate::engine::client::state(id).1.is_empty();
        let cur = coach.current();
        if cur == Some(Task::Attach) {
            coach.reach(Task::Attach);
        }
        if let Some(t @ (Task::Send | Task::Mention)) = cur
            && !busy
            && !sent.contains(&t)
        {
            let (to, text) = match (t, &reviewer) {
                (Task::Send, _) => (lead.clone(), format!("{TASK}\n\n{}\n- {MARKER}: {}", crate::stream::ATTACHMENTS, style_path(root).display())),
                (_, Some(r)) => (r.clone(), format!("@{r} {}", shortcut("review"))),
                (_, None) => (lead.clone(), format!("@{lead} {}", shortcut("polish"))),
            };
            if let Err(e) = prompt(&to, text) {
                println!("prompt: {e}");
                return 1;
            }
            sent.push(t);
        }
        std::thread::sleep(Duration::from_millis(300));
        for e in tail.read() {
            print(&e);
            coach.see(std::slice::from_ref(&e));
        }
    }
    println!("hello.md: {:?}", std::fs::read_to_string(repo.join("hello.md")).unwrap_or_default());
    if coach.current() != Some(Task::Changes) {
        println!("next task: {:?}", coach.current());
        return 1;
    }

    // quest 3: the IDE (mocked in the view), stage + commit hello.md only
    coach.reach(Task::Ide);
    // plain git, as the changes view runs it: the sandbox's own git config only
    let plain = |args: &[&str]| std::process::Command::new("git").arg("-C").arg(repo).args(args).stdin(std::process::Stdio::null()).output().map_err(|e| e.to_string()).and_then(|o| if o.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&o.stderr).trim().to_string()) });
    if let Err(e) = plain(&["add", "hello.md"]).and_then(|_| plain(&["commit", "-q", "-m", "hello.md says hi to the crew"])) {
        println!("commit: {e}");
        return 1;
    }
    coach.repo_at = None;
    coach.see_repo(repo);
    println!("README.md left unstaged: {}", git_out(repo, &["status", "--short"]).trim());
    if coach.current() != Some(Task::Sub) {
        println!("next task: {:?}", coach.current());
        return 1;
    }

    // quest 4: a sub-thread, started as the view's `>sub` starts it
    let side = reviewer.clone().unwrap_or_else(|| lead.clone());
    let task = format!("@{side} does README.md need a change?");
    let main = store::load(id);
    let mut kid = store::Thread::new(&task, &task, repo, vec![side.clone()]);
    kid.of = Some(id.to_string());
    if let Err(e) = kid.save() {
        println!("sub-thread: {e}");
        return 1;
    }
    let _ = crate::engine::client::call(id, &json!({"op": "sub_start", "id": kid.id, "title": task, "text": task}));
    let context = format!("[codehead: you run in a sub-thread of the thread “{}”: the same checkout ({}), 0 history of it. The message below is your whole task. Do it, then end with your result as your last message: the thread waits for you; it gets that message.]", main.map(|t| t.title).unwrap_or_default(), repo.display());
    let _ = crate::engine::client::call(&kid.id, &json!({"op": "prompt", "to": [side], "text": task, "context": context}));
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(60) && matches!(coach.current(), Some(Task::Sub | Task::SubDone)) {
        std::thread::sleep(Duration::from_millis(300));
        for e in tail.read() {
            if let Kind::SubThread { state, text, .. } = &e.kind {
                println!("sub-thread {state}: {text}");
            }
            print(&e);
            coach.see(std::slice::from_ref(&e));
        }
    }
    if coach.current() != Some(Task::AskPr) {
        println!("next task: {:?}", coach.current());
        return 1;
    }

    // quest 5: the PR link, the mock PR, its merge
    if let Err(e) = prompt(&lead, format!("@{lead} open a PR")) {
        println!("prompt: {e}");
        return 1;
    }
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(60) && coach.current() == Some(Task::AskPr) {
        std::thread::sleep(Duration::from_millis(300));
        for e in tail.read() {
            print(&e);
            coach.see(std::slice::from_ref(&e));
        }
    }
    let Some(r) = crate::gh::pr_urls(PR_URL).into_iter().next() else { return 1 };
    match crate::gh::fetch(&r) {
        Ok(p) => println!("mock PR: {} · {:?} · {} commits · +{} -{} · {:?}", p.title, p.state, p.commits.len(), p.additions, p.deletions, p.ready()),
        Err(e) => {
            println!("mock PR: {e}");
            return 1;
        }
    }
    coach.reach(Task::MergeView);
    let _ = crate::gh::run(&r, &crate::gh::Op::Merge("SQUASH".into(), false));
    coach.repo_at = None;
    coach.see_repo(repo);
    println!("mock PR after merge: {:?}", crate::gh::fetch(&r).map(|p| p.state));
    if coach.current() != Some(Task::Menu) {
        println!("next task: {:?}", coach.current());
        return 1;
    }

    // quest 7's thread: chore, run by the lead's steps
    coach.reach(Task::Flow);
    let Some(fid) = start_thread(&mut coach, repo, FLOW_TASK, Vec::new(), Vec::new(), &lead) else {
        println!("flow thread: 0 start");
        return 1;
    };
    let mut ftail = store::Tail::new(&fid);
    let start_at = std::time::Instant::now();
    while start_at.elapsed() < Duration::from_secs(90) && coach.current() == Some(Task::FlowRun) {
        std::thread::sleep(Duration::from_millis(300));
        for e in ftail.read() {
            print(&e);
            coach.see(std::slice::from_ref(&e));
        }
    }
    println!("next task: {:?}", coach.current());
    if coach.current() == Some(Task::Leave) { DONE } else { 1 }
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
        std::env::set_var("CODEHEAD_HOME_REPO", root.join("home-repo"));
        // the mocks (IDE, browser, GitHub): on, for the sandbox repo
        std::env::set_var("CODEHEAD_TUTORIAL", root.join("hello"));
        std::env::set_var("CODEHEAD_AGENT", "");
        std::env::set_var("CODEHEAD_REPO", "");
        std::env::remove_var("CODEHEAD_NOTES");
    }
    let code = match sandbox(&root, &look, &spec.crew) {
        Ok((id, seeded, repo)) => {
            let code = if check { headless(&id, &repo, &spec, &root) } else { show(&id, &seeded, &repo, &spec, &root) };
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

/// The style guide the 1st prompt attaches.
fn style_path(root: &Path) -> PathBuf {
    root.join("paste/pasted-text-1.txt")
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

// ---------- the mocks ----------

/// The tutorial's sandbox repo, when this process runs in the tutorial ($CODEHEAD_TUTORIAL):
/// 0 IDE opens, 0 browser opens, 0 GitHub call (a mock PR instead).
pub fn active() -> Option<PathBuf> {
    std::env::var_os("CODEHEAD_TUTORIAL").filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// The mock PR's merge marker in REPO.
fn merged_marker(repo: &Path) -> PathBuf {
    repo.join(".git/codehead-tutorial-merged")
}

/// The mock PR is merged.
fn mock_merged(repo: &Path) -> bool {
    merged_marker(repo).exists()
}

/// git's output in REPO ("" on a fail).
fn git_out(repo: &Path, args: &[&str]) -> String {
    std::process::Command::new("git").arg("-C").arg(repo).args(args).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default()
}

/// The 1st commit of REPO (the PR's base).
fn root_commit(repo: &Path) -> String {
    git_out(repo, &["rev-list", "--max-parents=0", "HEAD"]).lines().next().unwrap_or_default().to_string()
}

/// The mock PR R: the owner's commits of REPO since its 1st, checks green, the reviewer's ok.
pub fn mock_pr(r: &crate::gh::PrRef, repo: &Path) -> crate::gh::Pr {
    use crate::gh::{Check, Comment, Commit, Kind as CKind, Label, Pr, State, Thread, Verdict};
    let base = root_commit(repo);
    let range = format!("{base}..HEAD");
    let commits: Vec<Commit> = git_out(repo, &["log", "--reverse", "--format=%H%x09%h%x09%ct%x09%s", &range])
        .lines()
        .filter_map(|l| {
            let mut it = l.splitn(4, '\t');
            let (oid, short, at, headline) = (it.next()?, it.next()?, it.next()?, it.next()?);
            Some(Commit { oid: oid.into(), short: short.into(), date: at.parse::<u64>().unwrap_or(0), headline: headline.into() })
        })
        .collect();
    let (mut adds, mut dels, mut files) = (0, 0, 0);
    for l in git_out(repo, &["diff", "--numstat", &base, "HEAD"]).lines() {
        let mut it = l.split_whitespace();
        adds += it.next().and_then(|n| n.parse::<u64>().ok()).unwrap_or(0);
        dels += it.next().and_then(|n| n.parse::<u64>().ok()).unwrap_or(0);
        files += 1;
    }
    let reviewer = std::fs::read_to_string(repo.join(".git/codehead-tutorial-reviewer")).ok().map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).unwrap_or_else(|| "crew".into());
    let merged = mock_merged(repo);
    let now = crate::engine::now_ms() / 1000;
    let created = commits.last().map_or(now, |c| c.date);
    let check = |name: &str, secs| Check { name: name.into(), group: "CI".into(), verdict: Verdict::Pass, url: String::new(), required: true, secs, description: String::new() };
    Pr {
        pr: r.clone(),
        title: commits.first().map_or_else(|| "make hello.md say hi to the crew".into(), |c| c.headline.clone()),
        body: "hello.md greets the crew, in the style of the guide.\n\n- lower case, 1 line per idea\n- reviewed by the crew\n\n(the tutorial's mock PR: 0 GitHub calls)".into(),
        url: format!("https://github.com/{}/pull/{}", r.repo, r.number),
        state: if merged { State::Merged } else { State::Open },
        author: "you".into(),
        viewer: "you".into(),
        created,
        merged: merged.then_some(now),
        base: "main".into(),
        head: "task/hello".into(),
        head_oid: commits.last().map(|c| c.oid.clone()).unwrap_or_default(),
        additions: adds,
        deletions: dels,
        files,
        mergeable: "MERGEABLE".into(),
        merge_state: "CLEAN".into(),
        review: "APPROVED".into(),
        can_edit: true,
        method: "SQUASH".into(),
        labels: vec![Label { name: "docs".into(), color: "0075ca".into() }],
        reviewers: vec![(reviewer.clone(), "APPROVED".into())],
        threads: vec![Thread { root: Comment { kind: CKind::Review, id: 1, author: reviewer, body: "approved. 0 more nits.".into(), created, verdict: "APPROVED".into(), ..Default::default() }, ..Default::default() }],
        commits,
        checks: vec![check("lint", 14), check("test", 41)],
        ..Default::default()
    }
}

/// The mock PR's diff: COMMIT's own, else every commit since the 1st.
pub fn mock_diff(repo: &Path, commit: Option<&str>) -> Result<String, String> {
    Ok(match commit {
        Some(c) => git_out(repo, &["show", "--format=", c]),
        None => git_out(repo, &["diff", &root_commit(repo), "HEAD"]),
    })
}

/// An edit of the mock PR: a merge marks it merged; the rest: as if done.
pub fn mock_run(repo: &Path, op: &crate::gh::Op) -> Result<(), String> {
    if let crate::gh::Op::Merge(..) = op {
        std::fs::write(merged_marker(repo), "").map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ---------- the scripted agent ----------

/// The script's pace: slow enough to follow each step on screen.
const WORD: Duration = Duration::from_millis(70);
const TOOL: Duration = Duration::from_millis(1200);
const THINK: Duration = Duration::from_millis(1200);
/// Between 2 flow steps: the flow bar's move stays visible.
const STEP: Duration = Duration::from_millis(1500);

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
            std::thread::sleep(WORD);
        }
    }

    /// A tool call: shown, WORK done, its output shown. The output.
    fn tool(&mut self, title: &str, kind: &str, input: Value, work: impl FnOnce(&Path) -> String) -> String {
        self.next += 1;
        let id = format!("t{}", self.next);
        self.update(json!({"sessionUpdate": "tool_call", "toolCallId": id, "title": title, "kind": kind, "status": "in_progress", "rawInput": input}));
        std::thread::sleep(TOOL);
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
        self.read_file("hello.md");
    }

    fn read_file(&mut self, name: &str) {
        let path = self.cwd.join(name);
        let n = name.to_string();
        self.tool(&format!("Read {name}"), "read", json!({"file_path": path.display().to_string()}), move |d| std::fs::read_to_string(d.join(n)).unwrap_or_default());
    }

    /// A shell command shown, 0 run (the mock PR: 0 GitHub call).
    fn shell(&mut self, cmd: &str, output: &str) {
        let output = output.to_string();
        self.tool(cmd, "execute", json!({"command": cmd}), move |_| output);
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
            Some(next) => {
                std::thread::sleep(STEP);
                text = next;
            }
            None => break,
        }
    }
}

/// An owner message that asks for a pull request.
fn asks_pr(text: &str) -> bool {
    let t = text.to_lowercase();
    t.contains("pull request") || t.split(|c: char| !c.is_alphanumeric()).any(|w| w == "pr")
}

/// The turns of the script: ROLE, the prompt TEXT, turns done before.
fn turn(a: &mut Agent, role: &str, kv: &HashMap<String, String>, text: &str, n: u32) {
    let from_agent = text.lines().any(|l| l.starts_with("[from @"));
    let (builder, reviewer) = (kv.get("builder").cloned().unwrap_or_default(), kv.get("reviewer").cloned().unwrap_or_default());
    std::thread::sleep(THINK);
    if step_of(text).is_some() {
        return flow_turn(a, text);
    }
    // a sub-thread's task (any role): the check, the result as the last message
    if text.contains("you run in a sub-thread") {
        a.say("checking README.md against hello.md.");
        a.read_file("README.md");
        a.say("result: README.md names 0 greeting. 0 change needed for the PR.");
        return;
    }
    if !from_agent && asks_pr(text) {
        a.say("pushing the branch, opening the pull request.");
        a.shell("git push -u origin task/hello", "branch 'task/hello' set up to track 'origin/task/hello'.");
        a.shell("gh pr create --fill", PR_URL);
        a.say(&format!("PR opened: {PR_URL}"));
        return;
    }
    match role {
        "planner" => {
            a.say(&format!("plan: 1 change. hello.md greets the crew, in the style of the guide. {builder} does it."));
            a.hand(&builder, TASK);
        }
        "builder" if from_agent && !reviewer.is_empty() && text.contains(&format!("[from @{reviewer}]")) => {
            a.edit("hi crew.\nwelcome aboard.\n");
            a.say("fixed: a newline at the end, plus a welcome line.");
            a.hand(&reviewer, "fixed. 1 more look?");
        }
        "builder" if n == 0 => {
            a.say("reading the style guide first.");
            a.read_file("../paste/pasted-text-1.txt");
            a.edit("hi crew.");
            a.say("done: hello.md says hi to the crew. lower case, 1 line.");
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
            a.say(&format!("1 nit: the style guide wants a newline at the end. hello.md has 0. sent to {builder}."));
            a.hand(&builder, "hello.md: add a newline at the end, please (style guide).");
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
        crate::view::Peek { pane, menu, ide: false }
    }

    fn panel_text(c: &Coach) -> Vec<String> {
        panel_lines(c, 60).iter().map(|l| l.spans.iter().map(|s| s.content.to_string()).collect()).collect()
    }

    #[test]
    fn coach_follows_a_pair_through_every_quest() {
        use crate::view::Pane;
        let mut c = Coach::new(&crew(&["builder", "reviewer"]));
        assert_eq!(c.current(), Some(Task::Send));
        assert!(c.see(&[ev(None, prompt("owner", &["ADA"], TASK))]));
        assert!(c.see_view(&peek(Pane::Files, false)), "the attachment's click: the files view");
        assert!(c.text(c.current().unwrap()).0.contains("@NOVA #review"));

        // a message to the builder again: 0 step
        assert!(!c.see(&[ev(None, prompt("owner", &["ADA"], "more"))]));
        assert!(c.see(&[ev(None, prompt("owner", &["NOVA"], "@NOVA review the change"))]));

        // the reviewer's 1st turn end: not yet (the builder has not fixed)
        assert!(!c.see(&[ev(Some("NOVA"), prompt("NOVA", &["ADA"], "nit")), ev(Some("NOVA"), turn_end())]));
        assert!(c.see(&[ev(Some("ADA"), prompt("ADA", &["NOVA"], "fixed")), ev(Some("NOVA"), turn_end())]));

        // quest 3: the changes view, the IDE (mocked), then git's state (stage, commit)
        assert!(!c.see_view(&peek(Pane::Chat, true)), "the menu before the changes view: 0 step");
        assert!(c.see_view(&peek(Pane::Changes, false)));
        assert!(c.see_view(&crate::view::Peek { pane: Pane::Changes, menu: false, ide: true }));
        assert!(!c.finished());
        assert!(c.reach(Task::Commit));
        assert!(c.finished() && c.at_stop(), "the basics done: the owner may stop");
        assert!(panel_text(&c).iter().any(|l| l.contains("basics done")));

        // quest 4: the sub-thread's start and end
        assert!(c.see(&[ev(None, Kind::SubThread { id: "k".into(), title: "t".into(), state: "running".into(), text: String::new() })]));
        assert!(!c.at_stop(), "an advanced task done: 0 stop offer");
        assert!(c.see(&[ev(None, Kind::SubThread { id: "k".into(), title: "t".into(), state: "done".into(), text: String::new() })]));

        // quest 5: the PR link, the merge view, the merge
        assert!(c.see(&[ev(Some("ADA"), Kind::Text { text: format!("PR opened: {PR_URL}") })]));
        assert!(c.see_view(&peek(Pane::Merge, false)));
        assert_eq!(c.current(), Some(Task::Merged));
        assert!(c.reach(Task::Merged));

        // quest 6: ctrl+p, the switch; quest 7: the menu, n, a flow, the start, the flow's end
        assert!(c.see_view(&peek(Pane::Chat, true)));
        assert!(c.reach(Task::Switch));
        assert!(c.see_view(&peek(Pane::Chat, true)));
        assert!(c.reach(Task::New));
        assert!(c.tick(Task::Flow));
        assert!(c.reach(Task::Start));
        assert!(c.see(&[ev(None, Kind::Flow { op: "finished".into(), text: String::new() })]));
        assert_eq!(c.current(), Some(Task::Leave));
    }

    #[test]
    fn reach_marks_skipped_tasks_and_solo_has_0_watch() {
        let mut c = Coach::new(&crew(&["builder"]));
        assert!(!c.order().contains(&Task::Watch));
        c.see(&[ev(None, prompt("owner", &["ADA"], TASK))]);
        assert!(c.text(Task::Mention).0.contains("@ADA #polish"));
        // the files view skipped: the 2nd message counts all the same
        assert!(c.see(&[ev(None, prompt("owner", &["ADA"], "@ADA add a 2nd line"))]));
        assert!(c.is_done(Task::Attach) && c.is_done(Task::Mention));

        // n in the menu early: the tasks before count as done
        assert!(c.reach(Task::New));
        assert!(c.is_done(Task::Changes) && c.is_done(Task::Commit) && c.is_done(Task::Merged) && c.is_done(Task::Menu2));
        assert_eq!(c.current(), Some(Task::Flow));
        assert!(!c.reach(Task::New), "again: 0 change");
    }

    #[test]
    fn an_advanced_event_before_the_commit_counts_0() {
        let mut c = Coach::new(&crew(&["builder"]));
        assert!(!c.see(&[ev(Some("ADA"), Kind::Text { text: format!("PR opened: {PR_URL}") })]));
        assert!(!c.see(&[ev(None, Kind::SubThread { id: "k".into(), title: "t".into(), state: "running".into(), text: String::new() })]));
        assert_eq!(c.current(), Some(Task::Send));
    }

    #[test]
    fn panel_shows_the_current_quest_page_only() {
        let mut c = Coach::new(&crew(&["builder", "reviewer"]));
        c.reach(Task::Attach);
        let text = panel_text(&c);
        assert!(text.iter().any(|l| l.starts_with("2/8 work with the crew")), "{text:?}");
        assert!(text.iter().any(|l| l.contains("#name: a shortcut")), "the concept");
        assert!(text.iter().any(|l| l.starts_with("  › back to the chat. call NOVA in")));
        assert!(text.iter().any(|l| l.contains(" F1 · @ · # · ctrl+enter ")));
        assert!(text.iter().any(|l| l.contains("codehead.sh/docs/concepts/messaging/")));
        assert!(!text.iter().any(|l| l.contains("start a task") || l.contains("send the task")), "0 other quest: {text:?}");
        assert!(!text.iter().any(|l| l.contains("basics done")));
        // the dots: 1 done, this one, a gap before the advanced
        assert_eq!(text[0], " ✓ ◆ ○ │ ○ ○ ○ ○ ○");
    }

    /// The screen of a W x H terminal at C's state, as text rows.
    fn screen(c: &Coach, w: u16, h: u16) -> Vec<String> {
        let mut t = Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
        let (view, side) = layout(w, h);
        t.draw(|f| chrome(f, c, false, view, side)).unwrap();
        let b = t.backend().buffer();
        (0..h).map(|y| (0..w).map(|x| b[(x, y)].symbol().to_string()).collect()).collect()
    }

    #[test]
    fn the_longest_page_fits_the_smallest_window() {
        let mut c = Coach::new(&crew(&["planner", "builder", "reviewer"]));
        // quest 7: 5 concept lines, 5 tasks
        c.reach(Task::Start);
        let s = screen(&c, MIN_W, MIN_H);
        assert!(s.iter().any(|l| l.contains("more: codehead.sh/docs/concepts/flows/")), "{}", s.join("\n"));
        // the stop offer + quest 4
        let mut c = Coach::new(&crew(&["builder", "reviewer"]));
        c.reach(Task::Commit);
        let s = screen(&c, MIN_W, MIN_H);
        assert!(s.iter().any(|l| l.contains("basics done")) && s.iter().any(|l| l.contains("more: codehead.sh/docs/concepts/threads/")), "{}", s.join("\n"));
    }

    #[test]
    fn asks_pr_needs_the_word() {
        assert!(asks_pr("@ADA open a PR"));
        assert!(asks_pr("a pull request, please"));
        assert!(!asks_pr("print the prompt"));
    }

    #[test]
    fn mock_pr_shows_the_owners_commits_and_merges() {
        let d = std::env::temp_dir().join(format!("codehead-tmock-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("hello.md"), "hello\n").unwrap();
        git(&d, &["init", "-q", "-b", "main"]).unwrap();
        git(&d, &["add", "."]).unwrap();
        git(&d, &["commit", "-q", "-m", "hello"]).unwrap();
        std::fs::write(d.join("hello.md"), "hi crew.\nwelcome aboard.\n").unwrap();
        git(&d, &["commit", "-qam", "hello.md says hi"]).unwrap();
        let r = crate::gh::PrRef::new("crew/hello", 7);
        let p = mock_pr(&r, &d);
        assert_eq!((p.title.as_str(), p.commits.len(), p.additions, p.deletions, p.files), ("hello.md says hi", 1, 2, 1, 1));
        assert_eq!(p.ready(), crate::gh::Ready::Mergeable);
        assert!(mock_diff(&d, None).unwrap().contains("+welcome aboard."));
        mock_run(&d, &crate::gh::Op::Merge("SQUASH".into(), false)).unwrap();
        assert_eq!(mock_pr(&r, &d).state, crate::gh::State::Merged);
        let _ = std::fs::remove_dir_all(&d);
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
