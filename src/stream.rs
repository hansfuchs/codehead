//! The thread as the view shows it: the event log folded into items (messages, tool runs, asks),
//! then drawn as rows of a width. Tool calls of an agent between 2 of its messages are 1 run:
//! 1 row, opened by a click or ctrl+o.

use std::collections::{HashMap, HashSet};

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use serde_json::Value;

use crate::engine::store::{Event, Kind};
use crate::ask::Picker;
use crate::render::{self, Row};
use crate::theme;
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Debug)]
pub struct Call {
    pub id: String,
    pub name: String,
    pub input: Value,
    /// (ok, output) once the result came.
    pub output: Option<(bool, String)>,
}

#[derive(Clone, Debug)]
pub enum Item {
    /// FROM `owner` or an agent, to agents.
    Prompt { from: String, to: Vec<String>, text: String, ts: u64 },
    /// Agent text; THOUGHTS: the agent's thoughts right before it (KEY: their fold, 0: none).
    Text { agent: String, text: String, ts: u64, thoughts: Vec<String>, key: usize },
    /// Tool calls of an agent between 2 of its messages; THOUGHTS: (calls before it, text).
    Run { agent: String, key: usize, calls: Vec<Call>, start: u64, end: u64, thoughts: Vec<(usize, String)> },
    Note { agent: String, text: String, color: Color },
    /// An answered ask: what AGENT asked (QUESTION), the owner's ANSWER (allow false: declined /
    /// denied). ASKED, TS: the times of the ask and of the answer.
    Settled { agent: String, question: String, answer: String, allow: bool, permission: bool, asked: u64, ts: u64 },
    /// A subagent of AGENT (its master) ended: its report (done / failed) or why it ended (lost,
    /// stopped); a message of the master. SUB: its index in `Thread::subs`; KEY: the fold of a
    /// long report.
    Sub { agent: String, sub: usize, ts: u64, key: usize },
}

/// A subagent of an agent on the thread.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sub {
    pub id: String,
    /// Its master: the agent on the thread.
    pub master: String,
    /// The subagent that started it (None: the master).
    pub parent: Option<String>,
    pub description: String,
    pub agent_type: String,
    pub background: bool,
    pub prompt: String,
    pub start: u64,
    /// Claude Code: its transcript, its task id.
    pub transcript: String,
    pub task: String,
    /// None: running; else `done`, `failed`, `stopped`, `lost`.
    pub state: Option<String>,
    pub result: String,
    pub end: u64,
}

impl Sub {
    pub fn running(&self) -> bool {
        self.state.is_none()
    }
}

/// An open question or permission request.
#[derive(Clone, Debug)]
pub struct Ask {
    pub id: String,
    pub agent: String,
    pub tool: String,
    pub input: Value,
    /// When it came.
    pub ts: u64,
}

impl Ask {
    /// AskUserQuestion: its questions (text, header, options (label, description), multi).
    pub fn questions(&self) -> Vec<(String, String, Vec<(String, String)>, bool)> {
        if self.tool != "AskUserQuestion" {
            return Vec::new();
        }
        self.input["questions"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|q| {
                let s = |k: &str| q[k].as_str().unwrap_or_default().to_string();
                let opts = q["options"].as_array().into_iter().flatten().map(|o| (o["label"].as_str().unwrap_or_default().to_string(), o["description"].as_str().unwrap_or_default().to_string())).collect();
                (s("question"), s("header"), opts, q["multiSelect"].as_bool().unwrap_or(false))
            })
            .collect()
    }

    /// An ACP agent's own choices: (option id, label, kind).
    pub fn options(&self) -> Vec<(String, String, String)> {
        self.input["_options"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|o| {
                let s = |k: &str| o[k].as_str().unwrap_or_default().to_string();
                (s("id"), s("name"), s("kind"))
            })
            .collect()
    }

    /// What a permission request asks, in 1 line.
    pub fn summary(&self) -> String {
        if self.tool == "ExitPlanMode" {
            return "leave plan mode and start on the plan".into();
        }
        format!("{} {}", self.tool, call_summary(&self.tool, &self.input))
    }
}

#[derive(Default)]
pub struct Thread {
    pub items: Vec<Item>,
    pub asks: Vec<Ask>,
    /// Agents in a turn now.
    pub busy: HashSet<String>,
    /// Per agent: last status (state, note).
    pub status: HashMap<String, (String, String)>,
    /// Per agent: model of the last session.
    pub models: HashMap<String, String>,
    /// Open run index per agent (in items).
    open_run: HashMap<String, usize>,
    runs: usize,
    /// Per agent: thoughts not placed yet (the next tool call or message takes them).
    pending: HashMap<String, Vec<String>>,
    /// Fold keys of the messages with thoughts.
    thought_keys: usize,
    /// The subagents of the agents, in start order.
    pub subs: Vec<Sub>,
}

impl Thread {
    pub fn push(&mut self, e: &Event) {
        let agent = e.agent.clone().unwrap_or_default();
        match &e.kind {
            Kind::Prompt { from, to, text } => {
                self.open_run.remove(from);
                self.items.push(Item::Prompt { from: from.clone(), to: to.clone(), text: text.clone(), ts: e.ts });
            }
            Kind::Text { text } => {
                self.open_run.remove(&agent);
                self.text(agent, text.clone(), e.ts);
            }
            Kind::Thought { text } => self.pending.entry(agent).or_default().push(text.clone()),
            Kind::Tool { id, name, input } => {
                let call = Call { id: id.clone(), name: name.clone(), input: input.clone(), output: None };
                let before = self.pending.remove(&agent).unwrap_or_default();
                let i = match self.open_run.get(&agent).copied() {
                    Some(i) => i,
                    None => {
                        self.runs += 1;
                        self.open_run.insert(agent.clone(), self.items.len());
                        self.items.push(Item::Run { agent, key: self.runs, calls: Vec::new(), start: e.ts, end: e.ts, thoughts: Vec::new() });
                        self.items.len() - 1
                    }
                };
                if let Some(Item::Run { calls, end, thoughts, .. }) = self.items.get_mut(i) {
                    thoughts.extend(before.into_iter().map(|t| (calls.len(), t)));
                    calls.push(call);
                    *end = e.ts;
                }
            }
            Kind::ToolResult { id, ok, output } => {
                if let Some(i) = self.open_run.get(&agent).copied()
                    && let Some(Item::Run { calls, end, .. }) = self.items.get_mut(i)
                    && let Some(c) = calls.iter_mut().find(|c| &c.id == id)
                {
                    c.output = Some((*ok, output.clone()));
                    *end = e.ts;
                }
            }
            Kind::Ask { id, tool, input } => self.asks.push(Ask { id: id.clone(), agent, tool: tool.clone(), input: input.clone(), ts: e.ts }),
            Kind::Answer { id, allow, label } => {
                if let Some(i) = self.asks.iter().position(|a| &a.id == id) {
                    let a = self.asks.remove(i);
                    let qs = a.questions();
                    let question = if qs.is_empty() {
                        let mut q = a.summary();
                        if let Some(cmd) = a.input["command"].as_str() {
                            q.push_str(&format!("\n$ {cmd}"));
                        }
                        q
                    } else {
                        qs.iter().map(|(q, ..)| q.as_str()).collect::<Vec<_>>().join("\n\n")
                    };
                    self.items.push(Item::Settled { agent, question, answer: label.clone(), allow: *allow, permission: qs.is_empty(), asked: a.ts, ts: e.ts });
                }
            }
            Kind::TurnStart => {
                self.busy.insert(agent);
            }
            Kind::TurnEnd { error, .. } => {
                // thoughts at the turn's end: into its open run, else a message of their own
                if let Some(rest) = self.pending.remove(&agent) {
                    match self.open_run.get(&agent).and_then(|&i| self.items.get_mut(i)) {
                        Some(Item::Run { calls, thoughts, .. }) => thoughts.extend(rest.into_iter().map(|t| (calls.len(), t))),
                        _ => {
                            self.pending.insert(agent.clone(), rest);
                            self.text(agent.clone(), String::new(), e.ts);
                        }
                    }
                }
                self.open_run.remove(&agent);
                self.busy.remove(&agent);
                // a turn ends: its open asks are moot
                self.asks.retain(|a| a.agent != agent);
                match error.as_deref() {
                    None | Some("session ended") => {}
                    // the owner's esc: a note, 0 error hint
                    Some(crate::engine::host::INTERRUPTED) => self.items.push(Item::Note { agent, text: "interrupted".into(), color: theme::pal().muted }),
                    Some(err) => self.items.push(Item::Note { agent, text: err.to_string(), color: theme::pal().danger }),
                }
            }
            Kind::Session { model, .. } => {
                self.models.insert(agent, model.clone());
            }
            Kind::Status { state, note } => {
                self.status.insert(agent.clone(), (state.clone(), note.clone()));
                let text = if note.is_empty() { state.clone() } else { format!("{state} · {note}") };
                let color = match state.as_str() {
                    "blocked" => theme::pal().warn,
                    "done" => theme::pal().ok,
                    _ => theme::pal().muted,
                };
                self.items.push(Item::Note { agent, text, color });
            }
            Kind::Join => self.items.push(Item::Note { agent, text: "joined the thread".into(), color: theme::pal().muted }),
            Kind::Leave => {
                self.busy.remove(&agent);
                self.items.push(Item::Note { agent, text: "left the thread".into(), color: theme::pal().muted });
            }
            Kind::Error { message } => self.items.push(Item::Note { agent, text: message.clone(), color: theme::pal().danger }),
            Kind::Edits { summary } => self.items.push(Item::Note { agent, text: format!("got the edits since its last turn · {summary}"), color: theme::pal().muted }),
            Kind::SubStart { id, parent, description, agent_type, background, prompt } => {
                if !self.subs.iter().any(|s| &s.id == id) {
                    self.subs.push(Sub { id: id.clone(), master: agent, parent: parent.clone(), description: description.clone(), agent_type: agent_type.clone(), background: *background, prompt: prompt.clone(), start: e.ts, ..Default::default() });
                }
            }
            Kind::SubInfo { id, transcript, task } => {
                if let Some(s) = self.subs.iter_mut().find(|s| &s.id == id) {
                    if !transcript.is_empty() {
                        s.transcript = transcript.clone();
                    }
                    if !task.is_empty() {
                        s.task = task.clone();
                    }
                }
            }
            Kind::SubEnd { id, state, result } => {
                if let Some(i) = self.subs.iter().position(|s| &s.id == id && s.running()) {
                    let s = &mut self.subs[i];
                    s.state = Some(state.clone());
                    s.result = result.clone();
                    s.end = e.ts;
                    // the master's message: its own box ends here (the next text opens a new one)
                    self.open_run.remove(&agent);
                    self.runs += 1;
                    self.items.push(Item::Sub { agent, sub: i, ts: e.ts, key: self.runs });
                }
            }
        }
    }

    /// AGENT works: in a turn, or its subagents run.
    pub fn working(&self, agent: &str) -> bool {
        self.busy.contains(agent) || self.subs.iter().any(|s| s.running() && s.master == agent)
    }

    /// A message of AGENT: the thoughts before it go in.
    fn text(&mut self, agent: String, text: String, ts: u64) {
        let thoughts = self.pending.remove(&agent).unwrap_or_default();
        let key = if thoughts.is_empty() {
            0
        } else {
            self.thought_keys += 1;
            self.thought_keys
        };
        self.items.push(Item::Text { agent, text, ts, thoughts, key });
    }

    /// The owner's messages, oldest first (compose history).
    pub fn sent(&self) -> Vec<String> {
        self.items.iter().filter_map(|i| if let Item::Prompt { from, text, .. } = i { (from == "owner").then(|| text.clone()) } else { None }).collect()
    }

    /// The last messages of AGENT, oldest first, max. CHARS (a handoff).
    pub fn last_of(&self, agent: &str, chars: usize) -> String {
        let mut out: Vec<&str> = Vec::new();
        let mut n = 0;
        for i in self.items.iter().rev() {
            if let Item::Text { agent: a, text, .. } = i
                && a == agent
            {
                n += text.len();
                if n > chars && !out.is_empty() {
                    break;
                }
                out.push(text);
            }
        }
        out.reverse();
        out.join("\n\n")
    }
}

/// What a row of the stream does on a click.
#[derive(Clone, Debug, PartialEq)]
pub enum Hit {
    Run(usize),
    /// Pick up of lost subagent I (index in `Thread::subs`).
    PickUp(usize),
    /// The thoughts above message KEY.
    Thought(usize),
    /// Choice I of the open ask.
    Choice(usize),
}

/// How the stream shows the agents' thoughts (setting `thoughts`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Thoughts {
    /// 0 rows.
    #[default]
    Hidden,
    /// A folded row each (a click / ctrl+t opens it); in a run: a count, open with the run.
    Folded,
    /// Their full text, always.
    Open,
}

impl Thoughts {
    pub fn parse(s: &str) -> Option<Thoughts> {
        match s.trim() {
            "hidden" => Some(Thoughts::Hidden),
            "folded" => Some(Thoughts::Folded),
            "open" => Some(Thoughts::Open),
            _ => None,
        }
    }
}

/// What is open in the stream: runs (ALL_RUNS: every run), thoughts (ALL_THOUGHTS: every one).
#[derive(Clone, Copy)]
pub struct Fold<'a> {
    pub runs: &'a HashSet<usize>,
    pub all_runs: bool,
    pub thoughts: Thoughts,
    pub open_thoughts: &'a HashSet<usize>,
    pub all_thoughts: bool,
}

impl Fold<'_> {
    fn run(&self, key: usize) -> bool {
        self.all_runs || self.runs.contains(&key)
    }

    /// The thoughts above message KEY: open.
    fn thought(&self, key: usize) -> bool {
        self.thoughts == Thoughts::Open || self.all_thoughts || self.open_thoughts.contains(&key)
    }
}

/// Rows of the report of subagent SUB (a message of its master), BAR before each: `↳ subagent
/// “what” · done · 2m10s`, then the report (folded to 6 lines, KEY opens it) or why it ended (lost:
/// a `pick up` row: the master goes on from the subagent's context).
fn sub_rows(d: &mut Drawn, s: &Sub, sub: usize, key: usize, fold: &Fold, w: usize, bar: &[Span<'static>]) {
    let pal = theme::pal();
    let state = s.state.as_deref().unwrap_or("running");
    let color = match state {
        "done" => pal.ok,
        "stopped" => pal.muted,
        _ => pal.danger,
    };
    let secs = s.end.saturating_sub(s.start) / 1000;
    let took = if secs >= 60 { format!("{}m{:02}s", secs / 60, secs % 60) } else { format!("{secs}s") };
    let head = [bar.to_vec(), vec![
        Span::styled("↳ subagent ", theme::accent()),
        Span::styled(format!("“{}”", s.description), theme::bold()),
        Span::styled(" · ", theme::muted()),
        Span::styled(state.to_string(), Style::default().fg(color).add_modifier(Modifier::BOLD)),
        Span::styled(format!(" · {took}"), theme::muted()),
    ]]
    .concat();
    d.rows.push(Row::plain(Line::from(head)));
    d.rows.push(Row::plain(Line::from(bar.to_vec())));
    match state {
        "lost" | "stopped" => {
            let why = if state == "stopped" { "stopped by the owner".to_string() } else { format!("ended without a report: {}", s.result) };
            d.rows.extend(render::plain(&why, w, Style::default().fg(color), bar));
            if state == "lost" {
                let at = d.rows.len();
                let pre = [bar.to_vec(), vec![Span::styled("↻ ", theme::accent())]].concat();
                d.rows.extend(render::plain(&format!("pick up: {} goes on from this subagent's context (click)", s.master), w, theme::accent(), &pre));
                for i in at..d.rows.len() {
                    d.hits.insert(i, Hit::PickUp(sub));
                }
            }
        }
        _ => {
            let lines: Vec<&str> = s.result.trim_end().lines().collect();
            let open = fold.run(key);
            let long = lines.len() > 6;
            let text = if long && !open { lines[..6].join("\n") } else { s.result.trim_end().to_string() };
            d.rows.extend(memo::markdown(&text, w, bar));
            if long {
                let at = d.rows.len();
                let label = if open { "▾ fold the report".to_string() } else { format!("▸ {} more lines (click)", lines.len() - 6) };
                d.rows.push(Row::plain(Line::from([bar.to_vec(), vec![Span::styled(label, theme::muted())]].concat())));
                d.hits.insert(at, Hit::Run(key));
            }
        }
    }
}

/// Rows of the thoughts above a message (fold KEY), BAR before each: `▸ ∴ 1st line…` folded,
/// `▾ ∴ text` open (each thought wrapped). Clickable.
fn thought_rows(d: &mut Drawn, thoughts: &[String], key: usize, fold: &Fold, w: usize, bar: &[Span<'static>]) {
    if thoughts.is_empty() || fold.thoughts == Thoughts::Hidden {
        return;
    }
    let st = theme::muted().add_modifier(Modifier::ITALIC);
    let open = fold.thought(key);
    let at = d.rows.len();
    if open {
        for (i, t) in thoughts.iter().enumerate() {
            let mark = if i == 0 && fold.thoughts != Thoughts::Open { "▾ ∴ " } else { "  ∴ " };
            let first = [bar.to_vec(), vec![Span::styled(mark, theme::muted())]].concat();
            let mut rows = render::plain(t, w, st, &first);
            for r in rows.iter_mut().skip(1) {
                r.line.spans[bar.len()] = Span::raw("    ");
            }
            d.rows.extend(rows);
        }
    } else {
        let first = thoughts[0].lines().find(|l| !l.trim().is_empty()).unwrap_or_default();
        let more = if thoughts.len() > 1 { format!(" · {} thoughts", thoughts.len()) } else { String::new() };
        let room = w.saturating_sub(bar.iter().map(|s| s.content.width()).sum::<usize>() + 4 + more.width());
        let cut: String = if first.width() > room { first.chars().take(room.saturating_sub(1)).collect::<String>() + "…" } else { first.to_string() };
        d.rows.push(Row::plain(Line::from([bar.to_vec(), vec![Span::styled("▸ ∴ ", theme::muted()), Span::styled(cut, st), Span::styled(more, theme::muted())]].concat())));
    }
    if fold.thoughts != Thoughts::Open {
        for i in at..d.rows.len() {
            d.hits.insert(i, Hit::Thought(key));
        }
    }
}

/// The thoughts of a run (open: interleaved with its calls; Open setting: also under a folded run).
fn run_thought_rows(d: &mut Drawn, thoughts: &[(usize, String)], before: usize, w: usize, bar: &[Span<'static>]) {
    let st = theme::muted().add_modifier(Modifier::ITALIC);
    for (_, t) in thoughts.iter().filter(|(n, _)| *n == before) {
        let first = [bar.to_vec(), vec![Span::styled("∴ ", theme::muted())]].concat();
        let rest = [bar.to_vec(), vec![Span::raw("  ")]].concat();
        let mut rows = render::plain(t, w, st, &first);
        for r in rows.iter_mut().skip(1) {
            r.line.spans.splice(..bar.len() + 1, rest.clone());
        }
        d.rows.extend(rows);
    }
}

/// The drawn stream: rows, the clickable ones.
#[derive(Default)]
pub struct Drawn {
    pub rows: Vec<Row>,
    pub hits: HashMap<usize, Hit>,
    /// Where the view draws the working wave (3 cols): row, col, color (bubbles: the box of an
    /// agent at work).
    pub waves: Vec<(usize, u16, Color)>,
    /// Per item: its 1st row (the scroll anchor of a view scrolled back).
    pub starts: Vec<usize>,
}

/// How the stream draws messages (setting `messages`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Look {
    /// hell's: an agent's turn in a box (name, time in its top border), the owner's
    /// messages in a bubble on the right.
    #[default]
    Bubbles,
    /// A name badge, then the text behind a bar in the agent's color.
    Lines,
}

impl Look {
    pub fn parse(s: &str) -> Option<Look> {
        match s.trim() {
            "bubbles" => Some(Look::Bubbles),
            "lines" => Some(Look::Lines),
            _ => None,
        }
    }
}

/// Colors of the agents (name → color).
pub type Colors = HashMap<String, Color>;

fn color_of(colors: &Colors, name: &str) -> Color {
    colors.get(name).copied().unwrap_or(theme::pal().accent)
}

pub fn clock(ts: u64) -> String {
    let secs = (ts / 1000) as libc::time_t;
    // SAFETY: localtime_r writes only into TM.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&secs, &mut tm) };
    format!("{:02}:{:02}", tm.tm_hour, tm.tm_min)
}

fn badge(name: &str, color: Color, ts: u64) -> Vec<Span<'static>> {
    vec![
        Span::styled(format!(" {name} "), Style::default().bg(color).fg(theme::pal().on_accent).add_modifier(Modifier::BOLD)),
        Span::styled(format!(" {}", clock(ts)), theme::muted()),
    ]
}

/// 1 line of what a call does: the command, the file, the pattern.
pub fn call_summary(name: &str, input: &Value) -> String {
    let s = |k: &str| input[k].as_str().map(String::from);
    let v = match name {
        "Bash" => s("description").or_else(|| s("command")),
        "Read" | "Write" | "Edit" | "MultiEdit" | "NotebookEdit" => s("file_path").or_else(|| s("notebook_path")).map(|p| short_path(&p)),
        "Grep" | "Glob" => s("pattern"),
        "WebFetch" => s("url"),
        "WebSearch" => s("query"),
        "Task" | "Agent" => s("description"),
        "TodoWrite" => Some("todos".into()),
        _ if name.starts_with("mcp__codehead__send") => Some(format!("→ {}", match &input["to"] { Value::Array(a) => a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", "), v => v.as_str().unwrap_or_default().to_string() })),
        _ => input.as_object().and_then(|o| o.values().find_map(|v| v.as_str().map(String::from))),
    };
    v.unwrap_or_default().lines().next().unwrap_or_default().chars().take(120).collect()
}

fn short_path(p: &str) -> String {
    let cwd = std::env::current_dir().map(|c| c.display().to_string() + "/").unwrap_or_default();
    p.strip_prefix(&cwd).unwrap_or(p).to_string()
}

fn tool_label(name: &str) -> String {
    match name.strip_prefix("mcp__") {
        Some(rest) => rest.replace("__", ":"),
        None => name.to_string(),
    }
}

/// The run row: ` ◦ Bash: cargo test · Read: x.rs and 3 more · 5 calls · 12.0s`.
fn run_summary(calls: &[Call], secs: f64) -> String {
    let labels: Vec<String> = calls.iter().take(2).map(|c| {
        let s = call_summary(&c.name, &c.input);
        if s.is_empty() { tool_label(&c.name) } else { format!("{}: {s}", tool_label(&c.name)) }
    }).collect();
    let mut t = labels.join(" · ");
    if calls.len() > 2 {
        t.push_str(&format!(" and {} more", calls.len() - 2));
    }
    let n = calls.len();
    format!("{t} · {n} call{} · {secs:.1}s", if n == 1 { "" } else { "s" })
}

/// Draw T at WIDTH. OPEN: runs shown with their calls (ALL: every run). ASK_AT: the choice under
/// the cursor of the 1st open ask; PICKS: its picked choices (multi).
/// `needs harness: claude (Artifact tool)` in an agent's text: (harness, what for). The agents'
/// handoff rule asks for this line when 0 agent on the thread can do a step.
pub fn needs_harness(text: &str) -> Option<(String, String)> {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let c = RE.get_or_init(|| regex::Regex::new(r"(?i)\bneeds harness:\s*`?([a-z][a-z0-9_-]*)`?(?:\s*\(([^)]*)\))?").expect("valid regex")).captures(text)?;
    Some((c[1].to_ascii_lowercase(), c.get(2).map_or("", |m| m.as_str()).trim().to_string()))
}

/// A usage or rate limit in an error text.
pub fn usage_limit(msg: &str) -> bool {
    let msg = msg.to_ascii_lowercase();
    ["hit your", "usage limit", "rate limit", "rate_limit", "quota exceeded", "too many requests"].iter().any(|s| msg.contains(s))
}

/// The network in an error text.
pub fn offline(msg: &str) -> bool {
    ["ENOTFOUND", "ECONNREFUSED", "ECONNRESET", "ETIMEDOUT", "EAI_AGAIN", "Can't reach", "Connection error", "fetch failed"].iter().any(|t| msg.contains(t))
}

/// Agents (name, harness) other than SKIP on HARNESS (Some) / not on it (None: other harnesses).
fn agents_on<'a>(roster: &'a [(String, String)], harness: &str, same: bool, skip: &str) -> Vec<&'a (String, String)> {
    roster.iter().filter(|(n, h)| n != skip && (h == harness) == same).collect()
}

/// The hint rows of an error: what to do next, which agents can go on.
fn error_hint(agent: &str, text: &str, roster: &[(String, String)]) -> Vec<String> {
    let harness = roster.iter().find(|(n, _)| n == agent).map(|r| r.1.clone()).unwrap_or_default();
    if usage_limit(text) {
        let alt = agents_on(roster, &harness, false, agent);
        let mut v = vec![format!("{agent} stopped: {} usage limit reached.", crate::engine::harnesses::get(&harness).map_or(harness.as_str(), |h| h.label))];
        if alt.is_empty() {
            v.push("Agents on other harnesses: none".into());
        } else {
            v.push(format!("Agents on other harnesses: {}", alt.iter().map(|(n, h)| format!("@{n} ({h})")).collect::<Vec<_>>().join(", ")));
            v.push("Use @agent → ⇄ replace to continue.".into());
        }
        return v;
    }
    if text.contains("plan mode") || text.contains("ExitPlanMode") {
        return vec![format!("Fix: agents tab → {agent} → mode → autopilot, then send the message again.")];
    }
    vec![if offline(text) { "Once the connection is back, send a message to continue.".into() } else { "Send a message to continue.".into() }]
}

/// Draw T at WIDTH in LOOK. FOLD: what is open (runs, thoughts). ASK_AT: the choice under the
/// cursor of the 1st open ask; PICKS: its picked choices (multi).
pub fn draw(t: &Thread, look: Look, width: u16, colors: &Colors, roster: &[(String, String)], fold: &Fold, ask: &Picker) -> Drawn {
    // `@name` of an agent: in its color, in every message and note of this draw
    let mut marks: Vec<(String, Color)> = colors.iter().map(|(n, c)| (n.clone(), *c)).collect();
    marks.sort_by(|a, b| a.0.cmp(&b.0));
    render::set_marks(marks);
    let d = match look {
        Look::Lines => draw_lines(t, width, colors, roster, fold, ask),
        Look::Bubbles => draw_bubbles(t, width, colors, roster, fold, ask),
    };
    memo::sweep();
    d
}

/// Rows of the costly item bodies (markdown, open tool calls) of the last draw: a redraw (new
/// event, fold, ask key, resize back) renders only bodies with new input. Key: a hash of the
/// input, the width, the prefix spans and the palette. A body 0 draws used goes at the next sweep.
mod memo {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::rc::Rc;

    use ratatui::text::Span;

    use crate::render::Row;

    type Map = HashMap<u64, Rc<Vec<Row>>>;

    thread_local! {
        /// (used in this draw, used in the draw before).
        static MEMO: RefCell<(Map, Map)> = RefCell::default();
    }

    fn get(key: u64, make: impl FnOnce() -> Vec<Row>) -> Vec<Row> {
        let hit = MEMO.with_borrow_mut(|(cur, prev)| {
            if let Some(r) = cur.get(&key) {
                return Some(r.clone());
            }
            let r = prev.remove(&key)?;
            cur.insert(key, r.clone());
            Some(r)
        });
        let rows = hit.unwrap_or_else(|| {
            let r = Rc::new(make());
            MEMO.with_borrow_mut(|(cur, _)| cur.insert(key, r.clone()));
            r
        });
        rows.as_ref().clone()
    }

    fn key(what: &str, width: usize, prefix: &[Span<'static>], input: impl FnOnce(&mut DefaultHasher)) -> u64 {
        let mut h = DefaultHasher::new();
        (what, width, prefix, crate::theme::pal(), crate::render::marks_hash()).hash(&mut h);
        input(&mut h);
        h.finish()
    }

    pub fn markdown(text: &str, width: usize, prefix: &[Span<'static>]) -> Vec<Row> {
        get(key("md", width, prefix, |h| text.hash(h)), || crate::render::markdown(text, width, prefix))
    }

    pub fn call(c: &super::Call, width: usize, margin: &[Span<'static>]) -> Vec<Row> {
        let k = key("call", width, margin, |h| (&c.id, &c.name, c.input.to_string(), &c.output).hash(h));
        get(k, || super::call_rows(c, width, margin))
    }

    /// End of a draw: bodies this draw did not use go.
    pub fn sweep() {
        MEMO.with_borrow_mut(|(cur, prev)| *prev = std::mem::take(cur));
    }
}

/// ` · 2 thoughts` for a run's stats (0 while thoughts are hidden).
fn thought_count(thoughts: &[(usize, String)], fold: &Fold) -> String {
    match thoughts.len() {
        _ if fold.thoughts == Thoughts::Hidden => String::new(),
        0 => String::new(),
        1 => " · 1 thought".into(),
        n => format!(" · {n} thoughts"),
    }
}

fn draw_lines(t: &Thread, width: u16, colors: &Colors, roster: &[(String, String)], fold: &Fold, ask: &Picker) -> Drawn {
    let w = usize::from(width.max(10));
    let mut d = Drawn::default();
    let blank = |d: &mut Drawn| d.rows.push(Row::plain(Line::raw("")));
    let mut last_agent: Option<String> = None;
    for item in &t.items {
        d.starts.push(d.rows.len());
        match item {
            Item::Prompt { from, to, text, ts } if from == "owner" => {
                // the owner's message: a bubble on the right, solid hover bg, the time on its left
                blank(&mut d);
                let to_note = if to.len() > 1 || last_agent.as_deref() != to.first().map(String::as_str) { format!(" → {}", to.join(", ")) } else { String::new() };
                let (body, files) = split_attachments(text);
                let bw = (w * 3 / 4).max(20).min(w);
                let bg = Style::default().bg(theme::pal().hover);
                // markdown as the agents' text (bold, lists, code); the hover bg under every span
                let mut rows = memo::markdown(body.trim_end(), bw.saturating_sub(4), &[]);
                for r in &mut rows {
                    for s in &mut r.line.spans {
                        if s.style.bg.is_none() {
                            s.style = s.style.bg(theme::pal().hover);
                        }
                    }
                }
                rows.extend(files.iter().map(|f| Row::plain(Line::styled(format!("📎 {f}"), theme::muted().bg(theme::pal().hover)))));
                let inner = rows.iter().map(|r| r.line.width()).max().unwrap_or(0);
                let box_w = inner + 4;
                let left = w.saturating_sub(box_w);
                let stamp = format!("{}{to_note} ", clock(*ts));
                let n = rows.len();
                for (i, r) in rows.into_iter().enumerate() {
                    let pad = inner - r.line.width();
                    // left of the box: the time on the 1st row, a copy button on the last
                    let st = if i == 0 && stamp.width() < left { stamp.clone() } else { String::new() };
                    let copy = i + 1 == n && st.width() + 6 < left;
                    let used = st.width() + if copy { 5 } else { 0 };
                    let mut spans = vec![Span::raw(" ".repeat(left - used)), Span::styled(st, theme::muted())];
                    let mut copies: Vec<(u16, u16, String)> = Vec::new();
                    if copy {
                        spans.push(Span::styled("copy ", theme::accent()));
                        copies.push(((left - 5) as u16, (left - 1) as u16, text.trim_end().to_string()));
                    }
                    spans.push(Span::styled("  ", bg));
                    spans.extend(r.line.spans);
                    spans.push(Span::styled(" ".repeat(pad + 2), bg));
                    let shift = (left + 2) as u16;
                    copies.extend(r.copies.into_iter().map(|(a, b, u)| (a + shift, b + shift, u)));
                    d.rows.push(Row { wrap: r.wrap, line: Line::from(spans), links: r.links.into_iter().map(|(a, b, u)| (a + shift, b + shift, u)).collect(), copies });
                }
                last_agent = None;
            }
            Item::Prompt { from, to, text, ts } => {
                blank(&mut d);
                let c = color_of(colors, from);
                let mut head = badge(from, c, *ts);
                head.push(Span::styled(format!(" → {}", to.join(", ")), theme::muted()));
                d.rows.push(Row::plain(Line::from(head)));
                let bar = [Span::styled("┆ ", Style::default().fg(c))];
                d.rows.extend(render::plain(text.trim_end(), w, theme::muted(), &bar));
                last_agent = None;
            }
            // only thoughts, hidden: 0 rows
            Item::Text { text, thoughts, .. } if text.trim().is_empty() && (thoughts.is_empty() || fold.thoughts == Thoughts::Hidden) => {}
            Item::Text { agent, text, ts, thoughts, key } => {
                let c = color_of(colors, agent);
                if last_agent.as_deref() != Some(agent) {
                    blank(&mut d);
                    d.rows.push(Row::plain(Line::from(badge(agent, c, *ts))));
                } else {
                    blank(&mut d);
                }
                let bar = [Span::styled("│ ", Style::default().fg(c))];
                thought_rows(&mut d, thoughts, *key, fold, w, &bar);
                d.rows.extend(memo::markdown(text, w, &bar));
                if let Some((h, what)) = needs_harness(text) {
                    let label = crate::engine::harnesses::get(&h).map_or(h.as_str(), |x| x.label);
                    let what = if what.is_empty() { String::new() } else { format!(" for {what}") };
                    let cap = agents_on(roster, &h, true, agent);
                    let pre = [Span::styled("● ", Style::default().fg(theme::pal().warn))];
                    d.rows.extend(render::plain(&format!("{agent} needs a {label} agent{what}."), w, Style::default().fg(theme::pal().warn), &pre));
                    let list = if cap.is_empty() { "none".to_string() } else { cap.iter().map(|(n, _)| format!("@{n}")).collect::<Vec<_>>().join(", ") + " · @agent → + adds it" };
                    d.rows.extend(render::plain(&format!("Agents on {label}: {list}"), w, theme::muted(), &[Span::raw("  ")]));
                }
                last_agent = Some(agent.clone());
            }
            Item::Run { agent, key, calls, start, end, thoughts } => {
                let c = color_of(colors, agent);
                if last_agent.as_deref() != Some(agent) {
                    blank(&mut d);
                    d.rows.push(Row::plain(Line::from(badge(agent, c, *start))));
                    last_agent = Some(agent.clone());
                }
                let shown = fold.run(*key);
                let running = calls.iter().any(|c| c.output.is_none()) && t.busy.contains(agent);
                let mark = if shown { "▾ " } else if running { "◦ " } else { "▸ " };
                let failed = calls.iter().filter(|c| matches!(c.output, Some((false, _)))).count();
                let mut summary = run_summary(calls, end.saturating_sub(*start) as f64 / 1000.0);
                summary.push_str(&thought_count(thoughts, fold));
                if failed > 0 {
                    summary.push_str(&format!(" · {failed} failed"));
                }
                let think = fold.thoughts != Thoughts::Hidden && (shown || fold.thoughts == Thoughts::Open);
                let tbar = [Span::styled("│   ", Style::default().fg(c))];
                let row_at = d.rows.len();
                let pre = [Span::styled("│ ", Style::default().fg(c)), Span::styled(mark, theme::accent())];
                let rest = [Span::styled("│   ", Style::default().fg(c))];
                for (i, r) in render::plain(&summary, w, theme::muted(), &pre).into_iter().enumerate() {
                    let r = if i == 0 { r } else { Row { line: Line::from([rest.to_vec(), r.line.spans[2..].to_vec()].concat()), ..r } };
                    d.hits.insert(row_at + i, Hit::Run(*key));
                    d.rows.push(r);
                }
                if think && !shown {
                    for n in 0..=calls.len() {
                        run_thought_rows(&mut d, thoughts, n, w, &tbar);
                    }
                }
                if shown {
                    for (n, call) in calls.iter().enumerate() {
                        if think {
                            run_thought_rows(&mut d, thoughts, n, w, &tbar);
                        }
                        let head = format!("{} {}", tool_label(&call.name), call_summary(&call.name, &call.input));
                        let bar = [Span::styled("│   ", Style::default().fg(c))];
                        d.rows.extend(render::plain(&head, w, theme::text(), &bar));
                        d.rows.extend(memo::call(call, w, &[Span::styled("│     ", Style::default().fg(c))]));
                    }
                    if think {
                        run_thought_rows(&mut d, thoughts, calls.len(), w, &tbar);
                    }
                }
            }
            Item::Sub { agent, sub, ts, key } => {
                let c = color_of(colors, agent);
                blank(&mut d);
                d.rows.push(Row::plain(Line::from(badge(agent, c, *ts))));
                if let Some(s) = t.subs.get(*sub) {
                    sub_rows(&mut d, s, *sub, *key, fold, w, &[Span::styled("│ ", Style::default().fg(c))]);
                }
                last_agent = None;
            }
            Item::Note { agent, text, color } => {
                let c = color_of(colors, agent);
                let pre = [Span::styled("● ", Style::default().fg(*color)), Span::styled(format!("{agent} "), Style::default().fg(c))];
                d.rows.extend(render::plain(text, w, Style::default().fg(*color), &pre));
                if *color == theme::pal().danger {
                    for h in error_hint(agent, text, roster) {
                        d.rows.extend(render::plain(&h, w, theme::muted(), &[Span::raw("  ")]));
                    }
                }
                last_agent = None;
            }
            Item::Settled { agent, question, answer, allow, permission, .. } => {
                let text = match (permission, allow) {
                    (false, _) => format!("asked: {} · answered: {answer}", question.lines().next().unwrap_or_default()),
                    (true, true) => format!("{} · allowed", question.lines().next().unwrap_or_default()),
                    (true, false) => format!("{} · denied", question.lines().next().unwrap_or_default()),
                };
                let color = if *allow { theme::pal().ok } else { theme::pal().warn };
                let c = color_of(colors, agent);
                let pre = [Span::styled("● ", Style::default().fg(color)), Span::styled(format!("{agent} "), Style::default().fg(c))];
                d.rows.extend(render::plain(&text, w, Style::default().fg(color), &pre));
                last_agent = None;
            }
        }
    }
    // the 1st open ask: below everything, its choices clickable
    if let Some(a) = t.asks.first() {
        let c = color_of(colors, &a.agent);
        blank(&mut d);
        let edge = Style::default().fg(theme::pal().warn);
        let more = if t.asks.len() > 1 { format!(" · {} more waiting", t.asks.len() - 1) } else { String::new() };
        let what = if a.questions().is_empty() { "asks to run" } else { "asks" };
        d.rows.push(Row::plain(Line::from(vec![Span::styled("┃ ", edge), Span::styled(a.agent.clone(), Style::default().fg(c).add_modifier(Modifier::BOLD)), Span::styled(format!(" {what}{more}"), edge)])));
        ask_rows(&mut d, a, w, &[Span::styled("┃ ", edge)], ask);
    }
    d
}

/// The rows of open ask A (answered questions, the shown one, its rows: clickable, the hint),
/// BAR before each. ASK: the view's picker of A.
fn ask_rows(d: &mut Drawn, a: &Ask, w: usize, bar: &[Span<'static>], ask: &Picker) {
    // an ask of a subagent: whose it is
    if let Some(sub) = a.input["_subagent"].as_str() {
        d.rows.extend(render::plain(&format!("from subagent “{sub}” of {}", a.agent), w, theme::accent(), bar));
    }
    let qs = a.questions();
    let multi = qs.get(ask.q).is_some_and(|q| q.3);
    match qs.get(ask.q) {
        Some((q, header, _, _)) => {
            // the questions before / after: answered ones with their answer
            if qs.len() > 1 {
                let answered = ask.answered(a);
                let tabs: Vec<String> = qs
                    .iter()
                    .enumerate()
                    .map(|(i, (qq, h, ..))| {
                        let name = if h.is_empty() { format!("{}", i + 1) } else { h.clone() };
                        let done = answered.iter().any(|(x, _)| x == qq);
                        match (i == ask.q, done) {
                            (true, _) => format!("[{name}]"),
                            (false, true) => format!("{name} ✓"),
                            (false, false) => name,
                        }
                    })
                    .collect();
                d.rows.extend(render::plain(&format!("{} · {}/{}", tabs.join(" · "), ask.q + 1, qs.len()), w, theme::muted(), bar));
            } else if !header.is_empty() {
                d.rows.extend(render::plain(header, w, theme::muted(), bar));
            }
            d.rows.extend(render::plain(q, w, theme::bold(), bar));
        }
        None => {
            d.rows.extend(render::plain(&a.summary(), w, theme::bold(), bar));
            if let Some(cmd) = a.input["command"].as_str() {
                d.rows.extend(render::plain(&format!("$ {cmd}"), w, Style::default().fg(Color::Indexed(110)), bar));
            }
            if let Some(plan) = a.input["plan"].as_str() {
                d.rows.extend(render::markdown(plan, w, bar));
            }
        }
    }
    for (i, r) in ask.rows(a).into_iter().enumerate() {
        let cur = r.cursor && ask.focus;
        // multi: a box per row; single: a dot on the answer given before (←)
        let mark = match (multi, r.picked) {
            (true, true) => "[x] ",
            (true, false) => "[ ] ",
            (false, true) => "● ",
            (false, false) => "",
        };
        let st = if cur { theme::accent_bold() } else { theme::text() };
        let line = if r.other {
            let text = if r.label.is_empty() && !cur { "type an answer".to_string() } else { r.label.clone() };
            format!("{mark}{}. Other: {text}{}", i + 1, if cur { "▏" } else { "" })
        } else {
            format!("{mark}{}. {}", i + 1, r.label)
        };
        let at = d.rows.len();
        let pre = [bar.to_vec(), vec![Span::styled(if cur { "› " } else { "  " }, theme::accent())]].concat();
        let style = if r.other && r.label.is_empty() && !cur { theme::muted() } else { st };
        let mut rows = render::plain(&line, w, style, &pre);
        if !r.desc.is_empty() {
            rows.extend(render::plain(&format!("     {}", r.desc), w, theme::muted(), bar));
        }
        for k in 0..rows.len() {
            d.hits.insert(at + k, Hit::Choice(i));
        }
        d.rows.extend(rows);
    }
    d.rows.extend(render::plain(&ask.hint(a), w, theme::muted(), bar));
}

/// Most lines of a code block in an open run (the rest: `… n more lines`); of an output: the last ones.
const CALL_CODE_LINES: usize = 40;
const CALL_OUTPUT_LINES: usize = 12;

/// Language of FILE for the colors: its extension (`src/app.rs` → `rs`), else from the text.
fn file_lang(file: &str) -> String {
    std::path::Path::new(file).extension().and_then(|e| e.to_str()).unwrap_or_default().to_string()
}

/// Max. CALL_CODE_LINES of TEXT; the rest as a count.
fn head_lines(text: &str) -> (String, usize) {
    let lines: Vec<&str> = text.lines().collect();
    let more = lines.len().saturating_sub(CALL_CODE_LINES);
    (lines[..lines.len() - more].join("\n"), more)
}

/// `-` OLD lines, `+` NEW lines: an edit as a diff.
fn diff(old: &str, new: &str) -> String {
    let mut out: Vec<String> = old.lines().map(|l| format!("-{l}")).collect();
    out.extend(new.lines().map(|l| format!("+{l}")));
    out.join("\n")
}

/// The open rows of CALL below its head row, MARGIN before each: its code (the command of a Bash
/// call, the text of a Write, the diff of an Edit) and its output, as code blocks with colors.
fn call_rows(call: &Call, w: usize, margin: &[Span<'static>]) -> Vec<Row> {
    let mut out = Vec::new();
    let note = |n: usize, what: &str| render::plain(&format!("… {n} {what}"), w, theme::muted(), margin);
    let code = |out: &mut Vec<Row>, text: &str, lang: &str| {
        let (text, more) = head_lines(text);
        if !text.trim().is_empty() {
            out.extend(render::code_rows(&text, lang, w, margin, true, None));
        }
        if more > 0 {
            out.extend(note(more, "more lines"));
        }
    };
    let s = |k: &str| call.input[k].as_str();
    match call.name.as_str() {
        "Bash" => {
            if let Some(cmd) = s("command") {
                code(&mut out, cmd, "sh");
            }
        }
        "Write" => {
            if let (Some(file), Some(text)) = (s("file_path"), s("content")) {
                code(&mut out, text, &file_lang(file));
            }
        }
        "Edit" => {
            if let (Some(old), Some(new)) = (s("old_string"), s("new_string")) {
                code(&mut out, &diff(old, new), "diff");
            }
        }
        "MultiEdit" => {
            let edits = call.input["edits"].as_array().cloned().unwrap_or_default();
            let all: Vec<String> = edits.iter().map(|e| diff(e["old_string"].as_str().unwrap_or_default(), e["new_string"].as_str().unwrap_or_default())).collect();
            if !all.is_empty() {
                code(&mut out, &all.join("\n"), "diff");
            }
        }
        _ => {}
    }
    if let Some((ok, text)) = &call.output {
        let lines: Vec<&str> = text.lines().collect();
        let tail = lines.len().saturating_sub(CALL_OUTPUT_LINES);
        let text = lines[tail..].join("\n");
        if tail > 0 {
            out.extend(note(tail, "lines"));
        }
        if !text.trim().is_empty() {
            // output: plain text (json: colors); an error in the danger color
            let lang = if crate::code::language(&text, &[]) == "json" { "json" } else { "text" };
            let fg = if *ok { None } else { Some(theme::pal().danger) };
            out.extend(render::code_rows(&text, lang, w, margin, true, fg));
        }
    }
    out
}

/// Short label of 1 call for a bubble's run row: `run tests`, `read view.rs`, `search foo`.
fn call_label(c: &Call) -> String {
    let s = |k: &str| c.input[k].as_str().map(String::from);
    let base = |p: String| p.rsplit('/').next().unwrap_or_default().to_string();
    let lower = |t: String| {
        let mut ch = t.chars();
        ch.next().map_or_else(String::new, |f| f.to_lowercase().chain(ch).collect())
    };
    let v = match c.name.as_str() {
        "Bash" => s("description").map(lower).or_else(|| s("command").map(|c| format!("run {c}"))),
        "Read" => s("file_path").map(|p| format!("read {}", base(p))),
        "Write" => s("file_path").map(|p| format!("write {}", base(p))),
        "Edit" | "MultiEdit" => s("file_path").map(|p| format!("edit {}", base(p))),
        "NotebookEdit" => s("notebook_path").map(|p| format!("edit {}", base(p))),
        "Grep" | "Glob" => s("pattern").map(|p| format!("search {p}")),
        "WebFetch" => s("url").map(|u| format!("fetch {u}")),
        "WebSearch" => s("query").map(|q| format!("search the web for {q}")),
        "Task" | "Agent" => s("description").map(lower),
        "TodoWrite" => Some("update todos".into()),
        "ToolSearch" => s("query").map(|q| format!("load {}", q.trim_start_matches("select:").split(',').map(|t| t.rsplit("__").next().unwrap_or(t)).collect::<Vec<_>>().join(", "))),
        n if n.starts_with("mcp__") => {
            let mut parts = n.trim_start_matches("mcp__").splitn(2, "__");
            let (server, tool) = (parts.next().unwrap_or_default(), parts.next().unwrap_or_default());
            Some(format!("{} ({server})", tool.replace('_', " ")))
        }
        _ => None,
    };
    let v = v.unwrap_or_else(|| tool_label(&c.name));
    v.lines().next().unwrap_or_default().chars().take(80).collect()
}

/// The bubble's run rows: `Load skills_list, run tests, read x.rs and 5 more`, `8 calls · 20.7s`.
fn run_lines(calls: &[Call], secs: f64) -> (String, String) {
    let labels: Vec<String> = calls.iter().take(3).map(call_label).collect();
    let mut t = labels.join(", ");
    if calls.len() > 3 {
        t.push_str(&format!(" and {} more", calls.len() - 3));
    }
    let mut ch = t.chars();
    let t = ch.next().map_or_else(String::new, |f| f.to_uppercase().chain(ch).collect());
    let n = calls.len();
    (t, format!("{n} call{} · {secs:.1}s", if n == 1 { "" } else { "s" }))
}

/// The owner's message TEXT as a bubble on the right of a W wide stream: content-sized (max. 3/4),
/// the time (+ TO_NOTE) in its top border, attached files below it.
fn owner_bubble(d: &mut Drawn, w: usize, text: &str, to_note: &str, ts: u64) {
    let (body, files) = split_attachments(text);
    let body = body.trim_end();
    let cap = (w * 3 / 4 + 1).min(w);
    let stamp = clock(ts);
    let want = body.lines().map(|l| l.replace('\t', "    ").width()).max().unwrap_or(0) + 4;
    let bw = want.max(stamp.width() + 6).max(if to_note.is_empty() { 0 } else { to_note.width() + stamp.width() + 9 }).min(cap).max(8);
    let left = w - bw;
    let border = Style::default().fg(theme::pal().hover);
    let pad = Span::raw(" ".repeat(left));
    let lead = if to_note.is_empty() { Vec::new() } else { vec![Span::styled(to_note.to_string(), theme::muted())] };
    let (top, _) = top_border(bw, border, lead, false, &stamp);
    d.rows.push(Row { line: Line::from([vec![pad.clone()], top.line.spans].concat()), ..Default::default() });
    let bar = [pad.clone(), Span::styled("│ ", border)];
    let mut rows = vec![Row::plain(Line::from(bar.to_vec()))];
    rows.extend(memo::markdown(body, left + bw - 2, &bar));
    for mut r in rows {
        let rw = r.line.width();
        r.line.spans.push(Span::raw(" ".repeat((left + bw - 1).saturating_sub(rw))));
        r.line.spans.push(Span::styled("│", border));
        d.rows.push(r);
    }
    // the bottom border: a copy button on the right (the message as sent)
    let button = " copy ";
    if bw >= button.width() + 6 {
        let fill = bw - 2 - button.width() - 2;
        let at = (left + 1 + fill) as u16;
        d.rows.push(Row {
            line: Line::from(vec![pad, Span::styled(format!("└{}", "─".repeat(fill)), border), Span::styled(button, theme::accent()), Span::styled("──┘", border)]),
            copies: vec![(at, at + button.width() as u16, text.trim_end().to_string())],
            ..Default::default()
        });
    } else {
        d.rows.push(Row::plain(Line::from(vec![pad, Span::styled(format!("└{}┘", "─".repeat(bw - 2)), border)])));
    }
    for f in files {
        let name = f.rsplit('/').next().unwrap_or(f);
        let label = format!("📎 {name}");
        let lw = label.width();
        d.rows.push(Row::plain(Line::from(vec![Span::raw(" ".repeat(w.saturating_sub(lw))), Span::styled(label, theme::muted())])));
    }
}

/// A box under construction: its 1st row, the style of its border.
struct Open {
    start: usize,
    border: Style,
    agent: Option<String>,
}

/// Top border of width W: `┌─ ` LEFT ` ─────── ` RIGHT ` ─┐`. Returns the row and the
/// column after LEFT (where a wave goes).
fn top_border(w: usize, border: Style, left: Vec<Span<'static>>, wave: bool, right: &str) -> (Row, u16) {
    let mut spans = vec![Span::styled("┌─", border)];
    let lw: usize = left.iter().map(|s| s.content.width()).sum();
    let mut used = 2;
    if lw > 0 {
        spans.push(Span::styled(" ", border));
        spans.extend(left);
        used += 1 + lw;
    }
    let wave_at = used + 1;
    if wave {
        spans.push(Span::raw("    "));
        used += 4;
    }
    let rw = if right.is_empty() { 0 } else { right.width() + 2 };
    if lw > 0 && used < w {
        spans.push(Span::styled(" ", border));
        used += 1;
    }
    let fill = w.saturating_sub(used + rw + 2);
    spans.push(Span::styled("─".repeat(fill), border));
    if rw > 0 {
        spans.push(Span::styled(format!(" {right} "), theme::muted()));
    }
    spans.push(Span::styled("─┐", border));
    (Row::plain(Line::from(spans)), wave_at as u16)
}

fn draw_bubbles(t: &Thread, width: u16, colors: &Colors, roster: &[(String, String)], fold: &Fold, ask: &Picker) -> Drawn {
    let w = usize::from(width.max(12));
    // content width inside `│ ` … ` │`
    let inner = w - 2;
    let mut d = Drawn::default();
    let mut cur: Option<Open> = None;
    let close = |d: &mut Drawn, cur: &mut Option<Open>| {
        let Some(o) = cur.take() else { return };
        for r in &mut d.rows[o.start + 1..] {
            let rw = r.line.width();
            r.line.spans.push(Span::raw(" ".repeat((w - 1).saturating_sub(rw))));
            r.line.spans.push(Span::styled("│", o.border));
        }
        d.rows.push(Row::plain(Line::from(vec![Span::styled(format!("└{}┘", "─".repeat(w - 2)), o.border)])));
    };
    let gap = |d: &mut Drawn| {
        if !d.rows.is_empty() {
            d.rows.push(Row::plain(Line::raw("")));
        }
    };
    // last item of each agent at work: its box gets the wave, unless a message or an answer came
    // after it (a new turn: 0 output yet, the wave goes into a box of its own at the end)
    let last_of: HashMap<&str, usize> = t.items.iter().enumerate().filter_map(|(i, it)| match it {
        Item::Text { agent, .. } | Item::Run { agent, .. } | Item::Sub { agent, .. } => Some((agent.as_str(), i)),
        _ => None,
    }).collect();
    let turn_at = t.items.iter().rposition(|it| matches!(it, Item::Prompt { .. } | Item::Settled { .. }));
    let last_of: HashMap<&str, usize> = last_of.into_iter().filter(|&(_, l)| turn_at.is_none_or(|p| l > p)).collect();
    for (idx, item) in t.items.iter().enumerate() {
        d.starts.push(d.rows.len());
        match item {
            Item::Prompt { from, to, text, ts } if from == "owner" => {
                close(&mut d, &mut cur);
                gap(&mut d);
                let to_note = if to.len() > 1 { format!("→ {}", to.join(", ")) } else { String::new() };
                owner_bubble(&mut d, w, text, &to_note, *ts);
            }
            Item::Settled { agent, question, answer, allow, permission, asked, ts } => {
                // the question as the agent's message, the answer as the owner's
                close(&mut d, &mut cur);
                gap(&mut d);
                let c = color_of(colors, agent);
                let border = Style::default().fg(theme::desaturate(c, 0.4));
                let what = if *permission { " asked to run" } else { " asked" };
                let name = vec![Span::styled(agent.clone(), Style::default().fg(c).add_modifier(Modifier::BOLD)), Span::styled(what, theme::muted())];
                let (top, _) = top_border(w, border, name, false, &clock(*asked));
                cur = Some(Open { start: d.rows.len(), border, agent: None });
                d.rows.push(top);
                let bar = [Span::styled("│ ", border)];
                d.rows.push(Row::plain(Line::from(bar.to_vec())));
                d.rows.extend(render::plain(question, inner, theme::bold(), &bar));
                if !allow {
                    let st = if *permission { "denied" } else { "declined" };
                    d.rows.push(Row::plain(Line::from(bar.to_vec())));
                    d.rows.extend(render::plain(st, inner, Style::default().fg(theme::pal().warn), &bar));
                }
                close(&mut d, &mut cur);
                if *allow {
                    let answer = if *permission && answer.is_empty() { "allow" } else { answer.as_str() };
                    gap(&mut d);
                    owner_bubble(&mut d, w, answer, "", *ts);
                }
            }
            Item::Prompt { from, to, text, ts } => {
                close(&mut d, &mut cur);
                gap(&mut d);
                let c = color_of(colors, from);
                let border = Style::default().fg(theme::desaturate(c, 0.4));
                let name = vec![Span::styled(from.clone(), Style::default().fg(c).add_modifier(Modifier::BOLD)), Span::styled(format!(" → {}", to.join(", ")), theme::muted())];
                let (top, _) = top_border(w, border, name, false, &clock(*ts));
                cur = Some(Open { start: d.rows.len(), border, agent: None });
                d.rows.push(top);
                let bar = [Span::styled("│ ", border)];
                d.rows.push(Row::plain(Line::from(bar.to_vec())));
                d.rows.extend(render::plain(text.trim_end(), inner, theme::muted(), &bar));
                close(&mut d, &mut cur);
            }
            // only thoughts, hidden: 0 rows
            Item::Text { text, thoughts, .. } if text.trim().is_empty() && (thoughts.is_empty() || fold.thoughts == Thoughts::Hidden) => {}
            Item::Text { agent, ts, .. } | Item::Run { agent, start: ts, .. } | Item::Sub { agent, ts, .. } => {
                let c = color_of(colors, agent);
                let border = Style::default().fg(theme::desaturate(c, 0.4));
                let bar = [Span::styled("│ ", border)];
                if cur.as_ref().and_then(|o| o.agent.as_deref()) != Some(agent.as_str()) {
                    close(&mut d, &mut cur);
                    gap(&mut d);
                    // the box's time: its last item's
                    let mut end_ts = *ts;
                    for it in &t.items[idx..] {
                        match it {
                            Item::Text { agent: a, ts, .. } | Item::Sub { agent: a, ts, .. } if a == agent => end_ts = *ts,
                            Item::Run { agent: a, end, .. } if a == agent => end_ts = *end,
                            _ => break,
                        }
                    }
                    let busy = t.working(agent) && last_of.get(agent.as_str()).is_some_and(|&l| l >= idx && t.items[idx..=l].iter().all(|it| matches!(it, Item::Text { agent: a, .. } | Item::Run { agent: a, .. } | Item::Sub { agent: a, .. } if a == agent)));
                    let name = vec![Span::styled(agent.clone(), Style::default().fg(c).add_modifier(Modifier::BOLD))];
                    let (top, wave_at) = top_border(w, border, name, busy, &clock(end_ts));
                    if busy {
                        d.waves.push((d.rows.len(), wave_at, c));
                    }
                    cur = Some(Open { start: d.rows.len(), border, agent: Some(agent.clone()) });
                    d.rows.push(top);
                }
                d.rows.push(Row::plain(Line::from(bar.to_vec())));
                match item {
                    Item::Text { text, thoughts, key, .. } => {
                        // the thoughts before the message: a folded line on its top
                        thought_rows(&mut d, thoughts, *key, fold, inner, &bar);
                        if !text.trim().is_empty() {
                            d.rows.extend(memo::markdown(text, inner, &bar));
                        }
                        if let Some((h, what)) = needs_harness(text) {
                            let label = crate::engine::harnesses::get(&h).map_or(h.as_str(), |x| x.label);
                            let what = if what.is_empty() { String::new() } else { format!(" for {what}") };
                            let cap = agents_on(roster, &h, true, agent);
                            d.rows.push(Row::plain(Line::from(bar.to_vec())));
                            let pre = [bar[0].clone(), Span::styled("● ", Style::default().fg(theme::pal().warn))];
                            d.rows.extend(render::plain(&format!("{agent} needs a {label} agent{what}."), inner, Style::default().fg(theme::pal().warn), &pre));
                            let list = if cap.is_empty() { "none".to_string() } else { cap.iter().map(|(n, _)| format!("@{n}")).collect::<Vec<_>>().join(", ") + " · @agent → + adds it" };
                            d.rows.extend(render::plain(&format!("Agents on {label}: {list}"), inner, theme::muted(), &[bar[0].clone(), Span::raw("  ")]));
                        }
                    }
                    Item::Run { key, calls, start, end, thoughts, .. } => {
                        let shown = fold.run(*key);
                        let running = calls.iter().any(|c| c.output.is_none()) && t.busy.contains(agent);
                        let mark = if shown { "▾ " } else if running { "◦ " } else { "▸ " };
                        let failed = calls.iter().filter(|c| matches!(c.output, Some((false, _)))).count();
                        let (head, mut stats) = run_lines(calls, end.saturating_sub(*start) as f64 / 1000.0);
                        let think = fold.thoughts != Thoughts::Hidden && (shown || fold.thoughts == Thoughts::Open);
                        let tbar = [bar[0].clone(), Span::raw("  ")];
                        // `5 calls · 2 thoughts · 12.0s`
                        if let Some(at) = stats.rfind(" · ") {
                            stats.insert_str(at, &thought_count(thoughts, fold));
                        }
                        if failed > 0 {
                            stats.push_str(&format!(" · {failed} failed"));
                        }
                        let row_at = d.rows.len();
                        let pre = [bar[0].clone(), Span::styled(mark, theme::accent())];
                        let rest = [bar[0].clone(), Span::raw("  ")];
                        let mut rows = render::plain(&head, inner, theme::text(), &pre);
                        for r in rows.iter_mut().skip(1) {
                            r.line.spans[1] = Span::raw("  ");
                        }
                        rows.extend(render::plain(&stats, inner, theme::muted(), &rest));
                        for i in 0..rows.len() {
                            d.hits.insert(row_at + i, Hit::Run(*key));
                        }
                        d.rows.extend(rows);
                        if think && !shown {
                            for n in 0..=calls.len() {
                                run_thought_rows(&mut d, thoughts, n, inner, &tbar);
                            }
                        }
                        if shown {
                            for (n, call) in calls.iter().enumerate() {
                                if think {
                                    run_thought_rows(&mut d, thoughts, n, inner, &tbar);
                                }
                                let head = format!("{} {}", tool_label(&call.name), call_summary(&call.name, &call.input));
                                d.rows.extend(render::plain(&head, inner, theme::text(), &[bar[0].clone(), Span::raw("  ")]));
                                d.rows.extend(memo::call(call, inner, &[bar[0].clone(), Span::raw("    ")]));
                            }
                            if think {
                                run_thought_rows(&mut d, thoughts, calls.len(), inner, &tbar);
                            }
                        }
                    }
                    Item::Sub { sub, key, .. } => {
                        if let Some(s) = t.subs.get(*sub) {
                            sub_rows(&mut d, s, *sub, *key, fold, inner, &bar);
                        }
                    }
                    _ => {}
                }
            }
            Item::Note { agent, text, color } => {
                close(&mut d, &mut cur);
                gap(&mut d);
                let c = color_of(colors, agent);
                let pre = [Span::styled("● ", Style::default().fg(*color)), Span::styled(format!("{agent} "), Style::default().fg(c))];
                d.rows.extend(render::plain(text, w, Style::default().fg(*color), &pre));
                if *color == theme::pal().danger {
                    for h in error_hint(agent, text, roster) {
                        d.rows.extend(render::plain(&h, w, theme::muted(), &[Span::raw("  ")]));
                    }
                }
            }
        }
    }
    close(&mut d, &mut cur);
    // agents at work with 0 output in this turn yet: 0 box (their header card shows the work);
    // the box comes with the 1st output (live text included)
    // the 1st open ask: a yellow box below everything, its choices clickable
    if let Some(a) = t.asks.first() {
        gap(&mut d);
        let c = color_of(colors, &a.agent);
        let border = Style::default().fg(theme::pal().warn);
        let more = if t.asks.len() > 1 { format!(" · {} more waiting", t.asks.len() - 1) } else { String::new() };
        let what = if a.questions().is_empty() { "asks to run" } else { "asks" };
        let name = vec![Span::styled(a.agent.clone(), Style::default().fg(c).add_modifier(Modifier::BOLD)), Span::styled(format!(" {what}{more}"), border)];
        let (top, _) = top_border(w, border, name, false, "");
        cur = Some(Open { start: d.rows.len(), border, agent: None });
        d.rows.push(top);
        let bar = [Span::styled("│ ", border)];
        d.rows.push(Row::plain(Line::from(bar.to_vec())));
        ask_rows(&mut d, a, inner, &bar, ask);
        close(&mut d, &mut cur);
    }
    d
}

pub const ATTACHMENTS: &str = "Attachments (read these files):";

/// Prompt TEXT → (its body, the files attached under ATTACHMENTS).
pub fn split_attachments(text: &str) -> (&str, Vec<&str>) {
    match text.rfind(ATTACHMENTS) {
        Some(i) => (&text[..i], text[i + ATTACHMENTS.len()..].lines().map(|l| l.trim().trim_start_matches("- ")).filter(|l| !l.is_empty()).collect()),
        None => (text, Vec::new()),
    }
}

/// Width of the widest row (tests).
#[cfg(test)]
fn widest(d: &Drawn) -> usize {
    d.rows.iter().map(|r| r.line.width()).max().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The drawn rows on a test screen: (text per row, fg per cell).
    fn cells(d: &Drawn, w: u16) -> Vec<(String, Vec<Option<Color>>)> {
        use ratatui::widgets::Paragraph;
        let h = d.rows.len() as u16;
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h.max(1))).unwrap();
        term.draw(|f| f.render_widget(Paragraph::new(d.rows.iter().map(|r| r.line.clone()).collect::<Vec<_>>()), f.area())).unwrap();
        let b = term.backend().buffer().clone();
        (0..h).map(|y| ((0..w).map(|x| b[(x, y)].symbol().to_string()).collect(), (0..w).map(|x| Some(b[(x, y)].fg).filter(|c| *c != Color::Reset)).collect())).collect()
    }

    /// The fg of each cell of the 1st `@NAME` after row AFTER (0 match: panic).
    fn mention_fg(rows: &[(String, Vec<Option<Color>>)], name: &str, after: usize) -> Vec<Option<Color>> {
        let at = format!("@{name}");
        for (text, fg) in &rows[after..] {
            if let Some(byte) = text.find(&at) {
                let x = text[..byte].chars().count();
                return fg[x..x + at.chars().count()].to_vec();
            }
        }
        panic!("0 {at}: {:?}", rows.iter().map(|r| &r.0).collect::<Vec<_>>());
    }

    #[test]
    fn owner_messages_have_a_copy_button_in_both_looks() {
        let mut t = Thread::default();
        let msg = "fix **the deploy**\nsecond line";
        t.push(&ev(None, Kind::Prompt { from: "owner".into(), to: vec!["ada".into()], text: msg.into() }));
        let none = HashSet::new();
        let fold = Fold { runs: &none, all_runs: false, thoughts: Thoughts::Hidden, open_thoughts: &none, all_thoughts: false };
        for look in [Look::Bubbles, Look::Lines] {
            let d = draw(&t, look, 70, &Colors::new(), &[], &fold, &Picker::default());
            let row = d.rows.iter().find(|r| r.copies.iter().any(|c| c.2 == msg)).unwrap_or_else(|| panic!("{look:?}: 0 copy button"));
            let (a, b, _) = row.copies.iter().find(|c| c.2 == msg).unwrap().clone();
            let cells: String = row.line.spans.iter().flat_map(|s| s.content.chars()).collect();
            let hit: String = cells.chars().skip(usize::from(a)).take(usize::from(b - a)).collect();
            assert_eq!(hit.trim(), "copy", "{look:?}: the copy range sits on the button: {cells:?}");
        }
    }

    #[test]
    fn owner_messages_render_markdown_in_both_looks() {
        let mut t = Thread::default();
        t.push(&ev(None, Kind::Prompt { from: "owner".into(), to: vec!["ada".into()], text: "fix **the deploy** now\n- 1st `step`\n- 2nd step\nkeep this line".into() }));
        let none = HashSet::new();
        let fold = Fold { runs: &none, all_runs: false, thoughts: Thoughts::Hidden, open_thoughts: &none, all_thoughts: false };
        for look in [Look::Bubbles, Look::Lines] {
            let d = draw(&t, look, 70, &Colors::new(), &[], &fold, &Picker::default());
            let s = text(&d);
            assert!(s.contains("fix the deploy now") && !s.contains("**"), "{look:?} bold: 0 asterisks\n{s}");
            assert!(s.contains("• 1st step") && s.contains("• 2nd step") && !s.contains('`'), "{look:?} list + inline code\n{s}");
            // a line break of the source stays
            assert!(s.lines().any(|l| l.trim_end().trim_end_matches('│').trim().ends_with("keep this line")), "{look:?}\n{s}");
            let bold = d.rows.iter().flat_map(|r| r.line.spans.iter()).find(|sp| sp.content.contains("deploy")).unwrap();
            assert!(bold.style.add_modifier.contains(Modifier::BOLD), "{look:?} bold span");
        }
    }

    #[test]
    fn mentions_get_the_agent_color_in_every_message_and_both_looks() {
        let mut t = Thread::default();
        t.push(&ev(None, Kind::Prompt { from: "owner".into(), to: vec!["ada".into()], text: "@ada continue, cc @bob".into() }));
        t.push(&ev(Some("ada"), Kind::Text { text: "asked **@bob** here; `@bob` in code stays; mail@bob too\n\n```\n@bob in a block\n```".into() }));
        t.push(&ev(Some("ada"), Kind::Prompt { from: "ada".into(), to: vec!["bob".into()], text: "@bob take the build".into() }));
        t.push(&ev(Some("bob"), Kind::Status { state: "blocked".into(), note: "waits on @ada".into() }));
        let colors: Colors = [("ada".to_string(), Color::Rgb(200, 10, 10)), ("bob".to_string(), Color::Rgb(10, 10, 200))].into();
        let none = HashSet::new();
        let fold = Fold { runs: &none, all_runs: false, thoughts: Thoughts::Hidden, open_thoughts: &none, all_thoughts: false };
        let ada = Some(Color::Rgb(200, 10, 10));
        let bob = Some(Color::Rgb(10, 10, 200));
        for look in [Look::Bubbles, Look::Lines] {
            let d = draw(&t, look, 70, &colors, &[], &fold, &Picker::default());
            let rows = cells(&d, 70);
            let all = |fg: Vec<Option<Color>>, c: Option<Color>| fg.iter().all(|x| *x == c);
            // the owner's bubble
            assert!(all(mention_fg(&rows, "ada", 0), ada), "{look:?} owner @ada");
            let owner = rows.iter().position(|r| r.0.contains("@ada continue")).unwrap();
            assert!(all(mention_fg(&rows, "bob", owner), bob), "{look:?} owner @bob");
            // agent text: bold markdown too; inline code, a mail address, a code block: plain
            let text = rows.iter().position(|r| r.0.contains("asked")).unwrap();
            assert!(all(mention_fg(&rows, "bob", text), bob), "{look:?} text @bob");
            let code_row = &rows[text..].iter().find(|r| r.0.contains("in code")).unwrap();
            let x = code_row.0[..code_row.0.find("`@bob`").map_or_else(|| code_row.0.find("@bob in code").unwrap(), |b| b + 1)].chars().count();
            assert_ne!(code_row.1[x], bob, "{look:?} inline code: 0 mention color");
            let mail = rows.iter().find(|r| r.0.contains("mail@bob")).unwrap();
            let x = mail.0[..mail.0.find("mail@bob").unwrap()].chars().count() + 4;
            assert_ne!(mail.1[x], bob, "{look:?} mail@bob: 0 mention");
            let block = rows.iter().find(|r| r.0.contains("@bob in a block")).unwrap();
            let x = block.0[..block.0.find("@bob in a block").unwrap()].chars().count();
            assert_ne!(block.1[x], bob, "{look:?} code block: 0 mention");
            // agent → agent message, a status note
            let send = rows.iter().position(|r| r.0.contains("take the build")).unwrap();
            assert!(all(mention_fg(&rows, "bob", send), bob), "{look:?} agent message");
            let note = rows.iter().position(|r| r.0.contains("waits on")).unwrap();
            assert!(all(mention_fg(&rows, "ada", note), ada), "{look:?} note");
        }
        // an unknown name: 0 color
        let spans = render::mention_spans("@zed and @ada", theme::text(), &[("ada".into(), Color::Red)]);
        assert_eq!(spans.iter().filter(|s| s.style.fg == Some(Color::Red)).map(|s| s.content.as_ref()).collect::<Vec<_>>(), ["@ada"]);
    }

    fn ev(agent: Option<&str>, kind: Kind) -> Event {
        Event { ts: 1_000_000, agent: agent.map(String::from), kind }
    }

    fn text(d: &Drawn) -> String {
        d.rows.iter().map(|r| r.line.spans.iter().map(|s| s.content.as_ref()).collect::<String>().trim_end().to_string() + "\n").collect()
    }

    #[test]
    fn memo_redraw_shows_new_tool_output_and_equals_a_cold_draw() {
        let mut t = Thread::default();
        t.push(&ev(Some("ada"), Kind::Tool { id: "1".into(), name: "Bash".into(), input: json!({"command": "ls"}) }));
        let open: HashSet<usize> = [1].into();
        let none = HashSet::new();
        let fold = Fold { runs: &open, all_runs: false, thoughts: Thoughts::Hidden, open_thoughts: &none, all_thoughts: false };
        let first = text(&draw(&t, Look::Bubbles, 60, &Colors::new(), &[], &fold, &Picker::default()));
        assert!(!first.contains("fresh-output"), "{first}");
        t.push(&ev(Some("ada"), Kind::ToolResult { id: "1".into(), ok: true, output: "fresh-output".into() }));
        t.push(&ev(Some("ada"), Kind::Text { text: "a **b**".into() }));
        let warm = text(&draw(&t, Look::Bubbles, 60, &Colors::new(), &[], &fold, &Picker::default()));
        assert!(warm.contains("fresh-output"), "{warm}");
        let again = text(&draw(&t, Look::Bubbles, 60, &Colors::new(), &[], &fold, &Picker::default()));
        assert_eq!(warm, again);
        // a new width: new rows, 0 stale ones
        let narrow = draw(&t, Look::Bubbles, 30, &Colors::new(), &[], &fold, &Picker::default());
        assert!(widest(&narrow) <= 30);
    }

    #[test]
    fn folds_runs_and_asks() {
        let mut t = Thread::default();
        t.push(&ev(None, Kind::Prompt { from: "owner".into(), to: vec!["ada".into()], text: "fix it\n\nAttachments (read these files):\n- /tmp/a.png".into() }));
        t.push(&ev(Some("ada"), Kind::TurnStart));
        t.push(&ev(Some("ada"), Kind::Tool { id: "1".into(), name: "Bash".into(), input: json!({"command": "ls", "description": "list"}) }));
        t.push(&ev(Some("ada"), Kind::ToolResult { id: "1".into(), ok: true, output: "a\nb".into() }));
        t.push(&ev(Some("ada"), Kind::Tool { id: "2".into(), name: "Read".into(), input: json!({"file_path": "x.rs"}) }));
        t.push(&ev(Some("ada"), Kind::Text { text: "done **now**".into() }));
        t.push(&ev(Some("ada"), Kind::Ask { id: "q".into(), tool: "AskUserQuestion".into(), input: json!({"questions": [{"question": "color?", "header": "Color", "options": [{"label": "red", "description": ""}, {"label": "blue", "description": ""}], "multiSelect": false}]}) }));
        assert_eq!(t.items.len(), 3);
        assert!(t.busy.contains("ada"));
        let mut ask = Picker::default();
        ask.sync(t.asks.first());
        ask.at = 1;
        let d = draw(&t, Look::Lines, 60, &Colors::new(), &[], &Fold { runs: &HashSet::new(), all_runs: false, thoughts: Thoughts::Hidden, open_thoughts: &HashSet::new(), all_thoughts: false }, &ask);
        let s = text(&d);
        assert!(s.contains("  fix it"), "{s}");
        assert!(s.contains("📎 /tmp/a.png"), "{s}");
        assert!(s.contains("Bash: list · Read: x.rs · 2 calls"), "{s}");
        assert!(s.contains("│ done now"), "{s}");
        assert!(s.contains("┃ › 2. blue") && s.contains("┃   1. red"), "{s}");
        assert!(d.hits.values().any(|h| *h == Hit::Run(1)));
        assert!(widest(&d) <= 60);
        let open: HashSet<usize> = [1].into();
        let s = text(&draw(&t, Look::Lines, 60, &Colors::new(), &[], &Fold { runs: &open, all_runs: false, thoughts: Thoughts::Hidden, open_thoughts: &HashSet::new(), all_thoughts: false }, &Picker::default()));
        assert!(s.contains("│      sh") && s.contains("│      ls") && s.contains("│      b"), "{s}");
        t.push(&ev(Some("ada"), Kind::Answer { id: "q".into(), allow: true, label: "blue".into() }));
        t.push(&ev(Some("ada"), Kind::TurnEnd { cost: 0.1, input: 1, output: 1, cache_read: 0, cache_write: 0, ctx: 0, window: 0, ms: 1, error: None }));
        assert!(t.asks.is_empty() && t.busy.is_empty());
        assert!(text(&draw(&t, Look::Lines, 60, &Colors::new(), &[], &Fold { runs: &HashSet::new(), all_runs: false, thoughts: Thoughts::Hidden, open_thoughts: &HashSet::new(), all_thoughts: false }, &Picker::default())).contains("answered: blue"));
        assert_eq!(t.sent().len(), 1);
    }

    #[test]
    fn bubbles_box_turns_and_put_owner_messages_right() {
        let mut t = Thread::default();
        t.push(&ev(None, Kind::Prompt { from: "owner".into(), to: vec!["ada".into()], text: "fix it\n\nAttachments (read these files):\n- /tmp/a.png".into() }));
        t.push(&ev(Some("ada"), Kind::TurnStart));
        t.push(&ev(Some("ada"), Kind::Tool { id: "1".into(), name: "Bash".into(), input: json!({"command": "ls", "description": "List files"}) }));
        t.push(&ev(Some("ada"), Kind::ToolResult { id: "1".into(), ok: true, output: "a".into() }));
        t.push(&ev(Some("ada"), Kind::Text { text: "※ SUMMARY\nDONE. it works.".into() }));
        let d = draw(&t, Look::Bubbles, 50, &Colors::new(), &[], &Fold { runs: &HashSet::new(), all_runs: false, thoughts: Thoughts::Hidden, open_thoughts: &HashSet::new(), all_thoughts: false }, &Picker::default());
        let s = text(&d);
        let rows: Vec<&str> = s.lines().collect();
        // the owner's bubble: right, the time in its top border, the file name below it
        let top = rows.iter().position(|r| r.trim_start().starts_with("┌") && r.contains("─┐")).unwrap();
        assert!(rows[top].starts_with(' ') && rows[top].ends_with("─┐") && rows[top].contains(&clock(1_000_000)), "{s}");
        assert!(rows.iter().any(|r| r.ends_with("│ fix it   │") || (r.contains("│ fix it") && r.ends_with('│'))), "{s}");
        assert!(rows.iter().any(|r| r.ends_with("📎 a.png")), "{s}");
        // the agent's turn: 1 box, its name in the top border, the run on 2 rows, the text inside
        let b = rows.iter().position(|r| r.starts_with("┌─ ada")).unwrap();
        assert_eq!(rows[b].chars().count(), 50, "{s}");
        assert!(rows.iter().any(|r| r.starts_with("│ ▸ List files") && r.ends_with('│')), "{s}");
        assert!(rows.iter().any(|r| r.starts_with("│   1 call · 0.0s")), "{s}");
        assert!(rows.iter().any(|r| r.starts_with("│ ※ SUMMARY")) && rows.iter().any(|r| r.starts_with("│ DONE. it works.")), "{s}");
        assert!(rows.last().unwrap().starts_with("└") && rows.last().unwrap().ends_with('┘'), "{s}");
        assert!(widest(&d) <= 50);
        // at work: 1 wave in its top border
        assert_eq!(d.waves.len(), 1);
        assert_eq!(d.waves[0].0, b);
        assert!(d.hits.values().any(|h| *h == Hit::Run(1)));
        // an answered question: the agent's question in its box, the answer as the owner's bubble
        t.push(&ev(Some("ada"), Kind::Ask { id: "q".into(), tool: "AskUserQuestion".into(), input: json!({"questions": [{"question": "color?", "header": "Color", "options": [{"label": "red", "description": ""}], "multiSelect": false}]}) }));
        t.push(&ev(Some("ada"), Kind::Answer { id: "q".into(), allow: true, label: "red".into() }));
        let s = text(&draw(&t, Look::Bubbles, 50, &Colors::new(), &[], &Fold { runs: &HashSet::new(), all_runs: false, thoughts: Thoughts::Hidden, open_thoughts: &HashSet::new(), all_thoughts: false }, &Picker::default()));
        let rows: Vec<&str> = s.lines().collect();
        let q = rows.iter().position(|r| r.starts_with("┌─ ada asked")).expect(&s);
        assert!(rows[q + 2].starts_with("│ color?"), "{s}");
        assert!(rows[q + 4..].iter().any(|r| r.trim_end().ends_with("│ red    │") || (r.contains("│ red") && r.ends_with('│') && r.starts_with(' '))), "{s}");
        // declined: 0 answer bubble
        t.push(&ev(Some("ada"), Kind::Ask { id: "p".into(), tool: "Bash".into(), input: json!({"command": "rm x"}) }));
        t.push(&ev(Some("ada"), Kind::Answer { id: "p".into(), allow: false, label: "deny".into() }));
        let s = text(&draw(&t, Look::Bubbles, 50, &Colors::new(), &[], &Fold { runs: &HashSet::new(), all_runs: false, thoughts: Thoughts::Hidden, open_thoughts: &HashSet::new(), all_thoughts: false }, &Picker::default()));
        // ada still at work, 0 output since: 0 empty box below the declined ask
        let tail: Vec<&str> = s.lines().rev().take(8).collect();
        assert!(tail.iter().any(|r| r.starts_with("│ denied")) && tail[0].starts_with('└'), "{s}");
        assert!(!tail.iter().take_while(|r| !r.starts_with("│ denied")).any(|r| r.starts_with("┌─ ada")), "{s}");
    }

    #[test]
    fn a_new_turn_shows_no_box_until_its_1st_output() {
        let mut t = Thread::default();
        t.push(&ev(Some("ada"), Kind::TurnStart));
        t.push(&ev(Some("ada"), Kind::Text { text: "first answer".into() }));
        t.push(&ev(Some("ada"), Kind::TurnEnd { cost: 0.0, input: 0, output: 0, cache_read: 0, cache_write: 0, ctx: 0, window: 0, ms: 1, error: None }));
        t.push(&ev(None, Kind::Prompt { from: "owner".into(), to: vec!["ada".into()], text: "next".into() }));
        t.push(&ev(Some("ada"), Kind::TurnStart));
        let fold = Fold { runs: &HashSet::new(), all_runs: false, thoughts: Thoughts::Hidden, open_thoughts: &HashSet::new(), all_thoughts: false };
        let d = draw(&t, Look::Bubbles, 50, &Colors::new(), &[], &fold, &Picker::default());
        let s = text(&d);
        let rows: Vec<&str> = s.lines().collect();
        let bubble = rows.iter().position(|r| r.contains("next")).unwrap();
        // 0 output yet: 0 empty box below the owner's message, 0 wave on the answer above it
        assert!(d.waves.is_empty(), "{s}");
        assert!(!rows[bubble..].iter().any(|r| r.starts_with("┌─ ada")), "{s}");
        assert_eq!(rows.iter().filter(|r| r.starts_with("┌─ ada")).count(), 1, "{s}");
        // the 1st output of the turn: the wave moves into its box, 0 empty box
        t.push(&ev(Some("ada"), Kind::Text { text: "second".into() }));
        let d = draw(&t, Look::Bubbles, 50, &Colors::new(), &[], &fold, &Picker::default());
        let s = text(&d);
        let rows: Vec<&str> = s.lines().collect();
        assert_eq!(d.waves.len(), 1, "{s}");
        assert!(rows[d.waves[0].0 + 2].contains("second"), "{s}");
    }

    #[test]
    fn interrupted_turns_are_no_errors() {
        let mut t = Thread::default();
        let end = |error: &str| Kind::TurnEnd { cost: 0.0, input: 0, output: 0, cache_read: 0, cache_write: 0, ctx: 0, window: 0, ms: 1, error: Some(error.into()) };
        t.push(&ev(Some("ada"), Kind::TurnStart));
        t.push(&ev(Some("ada"), end(crate::engine::host::INTERRUPTED)));
        assert!(matches!(t.items.last(), Some(Item::Note { text, color, .. }) if text == "interrupted" && *color == theme::pal().muted));
        t.push(&ev(Some("ada"), Kind::TurnStart));
        t.push(&ev(Some("ada"), end("error_during_execution")));
        assert!(matches!(t.items.last(), Some(Item::Note { color, .. }) if *color == theme::pal().danger));
    }

    #[test]
    fn open_runs_show_code_blocks() {
        let mut t = Thread::default();
        t.push(&ev(Some("ada"), Kind::TurnStart));
        let cmd = "bun -e 'const d = await Bun.file(\"/very/long/path/to/some/report/data/file.json\").json(); console.log(d)'";
        t.push(&ev(Some("ada"), Kind::Tool { id: "1".into(), name: "Bash".into(), input: json!({"command": cmd, "description": "Show data"}) }));
        t.push(&ev(Some("ada"), Kind::ToolResult { id: "1".into(), ok: true, output: "{\"a\": 1}".into() }));
        t.push(&ev(Some("ada"), Kind::Tool { id: "2".into(), name: "Edit".into(), input: json!({"file_path": "src/x.rs", "old_string": "let a = 1;", "new_string": "let a = 2;"}) }));
        t.push(&ev(Some("ada"), Kind::ToolResult { id: "2".into(), ok: false, output: "no match".into() }));
        let open: HashSet<usize> = [1].into();
        for look in [Look::Bubbles, Look::Lines] {
            let d = draw(&t, look, 60, &Colors::new(), &[], &Fold { runs: &open, all_runs: false, thoughts: Thoughts::Hidden, open_thoughts: &HashSet::new(), all_thoughts: false }, &Picker::default());
            let s = text(&d);
            if std::env::var_os("SHOW").is_some() {
                eprintln!("{s}");
            }
            // the command: a sh block, wrapped (0 chars cut), with a copy button
            assert!(s.contains(" sh ") && s.contains("copy") && s.contains("Bun.file") && s.contains("data/file.json"), "{s}");
            assert!(s.contains("-let a = 1;") && s.contains("+let a = 2;") && s.contains(" diff "), "{s}");
            assert!(s.contains(" json ") && s.contains("no match"), "{s}");
            assert!(widest(&d) <= 60, "{s}");
            // the copy button holds the whole command
            assert!(d.rows.iter().any(|r| r.copies.iter().any(|c| c.2 == cmd)));
        }
    }

    #[test]
    fn thoughts_fold_into_runs_and_messages() {
        let mut t = Thread::default();
        t.push(&ev(Some("ada"), Kind::TurnStart));
        t.push(&ev(Some("ada"), Kind::Thought { text: "Look at the logs first.".into() }));
        t.push(&ev(Some("ada"), Kind::Tool { id: "1".into(), name: "Bash".into(), input: json!({"command": "ls", "description": "List files"}) }));
        t.push(&ev(Some("ada"), Kind::Thought { text: "Cause found: CI=true missing, so I am fixing it now.".into() }));
        t.push(&ev(Some("ada"), Kind::Text { text: "Fixed.".into() }));
        // the 1st thought: in the run before its call; the 2nd: on the message
        let Item::Run { thoughts, .. } = &t.items[0] else { panic!() };
        assert_eq!(thoughts, &vec![(0, "Look at the logs first.".to_string())]);
        let Item::Text { thoughts, key, .. } = &t.items[1] else { panic!() };
        assert_eq!((thoughts.len(), *key), (1, 1));
        let none = HashSet::new();
        let fold = |t: &Thread, mode: Thoughts, open: &HashSet<usize>| -> String {
            let f = Fold { runs: &none, all_runs: false, thoughts: mode, open_thoughts: open, all_thoughts: false };
            text(&draw(t, Look::Bubbles, 50, &Colors::new(), &[], &f, &Picker::default()))
        };
        // hidden: 0 thought rows, 0 count
        let s = fold(&t, Thoughts::Hidden, &none);
        assert!(!s.contains('∴') && !s.contains("thought"), "{s}");
        // folded: a count on the run, 1 folded line on the message (cut to the box)
        let s = fold(&t, Thoughts::Folded, &none);
        assert!(s.contains("1 call · 1 thought · "), "{s}");
        let line = s.lines().find(|r| r.contains("▸ ∴")).expect(&s);
        assert!(line.starts_with("│ ▸ ∴ Cause found") && line.contains('…') && line.ends_with('│'), "{s}");
        assert!(s.lines().position(|r| r.contains("▸ ∴")) < s.lines().position(|r| r.contains("Fixed.")), "{s}");
        // a click opens it: the full text, wrapped
        let open: HashSet<usize> = [1].into();
        let s = fold(&t, Thoughts::Folded, &open);
        assert!(s.contains("▾ ∴ Cause found") && s.contains("now."), "{s}");
        // open: every thought, the run's too (under its folded row)
        let s = fold(&t, Thoughts::Open, &none);
        assert!(s.contains("∴ Look at the logs first.") && s.contains("now."), "{s}");
        // a turn that ends on a thought, 0 text: a message of its own; hidden: 0 rows
        t.push(&ev(Some("ada"), Kind::Thought { text: "Nothing left.".into() }));
        t.push(&ev(Some("ada"), Kind::TurnEnd { cost: 0.0, input: 0, output: 0, cache_read: 0, cache_write: 0, ctx: 0, window: 0, ms: 1, error: None }));
        assert!(matches!(&t.items[2], Item::Text { text, .. } if text.is_empty()));
        assert!(!fold(&t, Thoughts::Hidden, &none).contains("Nothing left"));
        assert!(fold(&t, Thoughts::Folded, &none).contains("▸ ∴ Nothing left."));
    }
}

/// Draw time of a thread on disk: `CODEHEAD_BENCH=<thread id> cargo test --release bench_draw -- --ignored --nocapture`.
#[cfg(test)]
mod bench {
    use super::*;

    #[test]
    #[ignore]
    fn bench_draw() {
        let Ok(id) = std::env::var("CODEHEAD_BENCH") else { return };
        let mut t = Thread::default();
        for e in crate::engine::store::events(&id) {
            t.push(&e);
        }
        let all: HashSet<usize> = (0..=t.runs).collect();
        let none = HashSet::new();
        for (name, runs) in [("folded", &none), ("open runs", &all)] {
            for look in [Look::Bubbles, Look::Lines] {
                let fold = Fold { runs, all_runs: false, thoughts: Thoughts::Folded, open_thoughts: &none, all_thoughts: false };
                let mut ms = Vec::new();
                let mut rows = 0;
                for _ in 0..3 {
                    let at = std::time::Instant::now();
                    let d = draw(&t, look, 120, &Colors::new(), &[], &fold, &Picker::default());
                    ms.push(at.elapsed().as_secs_f64() * 1000.0);
                    rows = d.rows.len();
                }
                eprintln!("{look:?} {name}: {} items, {rows} rows, ms per draw: {:.1} / {:.1} / {:.1}", t.items.len(), ms[0], ms[1], ms[2]);
            }
        }
    }
}
