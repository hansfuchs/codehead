//! The thread as the view shows it: the event log folded into items (messages, tool runs, asks),
//! then drawn as rows of a width. Tool calls of an agent between 2 of its messages are 1 run:
//! 1 row, opened by a click or ctrl+o.

use std::collections::{HashMap, HashSet};

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use serde_json::Value;

use crate::engine::store::{Event, Kind};
use crate::render::{self, Row};
use crate::theme;

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
    Text { agent: String, text: String, ts: u64 },
    Run { agent: String, key: usize, calls: Vec<Call>, start: u64, end: u64 },
    Note { agent: String, text: String, color: Color },
}

/// An open question or permission request.
#[derive(Clone, Debug)]
pub struct Ask {
    pub id: String,
    pub agent: String,
    pub tool: String,
    pub input: Value,
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
                self.items.push(Item::Text { agent, text: text.clone(), ts: e.ts });
            }
            Kind::Tool { id, name, input } => {
                let call = Call { id: id.clone(), name: name.clone(), input: input.clone(), output: None };
                match self.open_run.get(&agent).copied() {
                    Some(i) => {
                        if let Some(Item::Run { calls, end, .. }) = self.items.get_mut(i) {
                            calls.push(call);
                            *end = e.ts;
                        }
                    }
                    None => {
                        self.runs += 1;
                        self.open_run.insert(agent.clone(), self.items.len());
                        self.items.push(Item::Run { agent, key: self.runs, calls: vec![call], start: e.ts, end: e.ts });
                    }
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
            Kind::Ask { id, tool, input } => self.asks.push(Ask { id: id.clone(), agent, tool: tool.clone(), input: input.clone() }),
            Kind::Answer { id, allow, label } => {
                if let Some(i) = self.asks.iter().position(|a| &a.id == id) {
                    let a = self.asks.remove(i);
                    let text = match (a.questions().first(), allow) {
                        (Some((q, ..)), _) => format!("asked: {q} · answered: {label}"),
                        (None, true) => format!("{} · allowed", a.summary()),
                        (None, false) => format!("{} · denied", a.summary()),
                    };
                    self.items.push(Item::Note { agent, text, color: if *allow { theme::OK } else { theme::WARN } });
                }
            }
            Kind::TurnStart => {
                self.busy.insert(agent);
            }
            Kind::TurnEnd { error, .. } => {
                self.open_run.remove(&agent);
                self.busy.remove(&agent);
                // a turn ends: its open asks are moot
                self.asks.retain(|a| a.agent != agent);
                if let Some(err) = error.as_deref().filter(|e| *e != "session ended") {
                    self.items.push(Item::Note { agent, text: err.to_string(), color: theme::DANGER });
                }
            }
            Kind::Session { model, .. } => {
                self.models.insert(agent, model.clone());
            }
            Kind::Status { state, note } => {
                self.status.insert(agent.clone(), (state.clone(), note.clone()));
                let text = if note.is_empty() { state.clone() } else { format!("{state} · {note}") };
                let color = match state.as_str() {
                    "blocked" => theme::WARN,
                    "done" => theme::OK,
                    _ => theme::MUTED,
                };
                self.items.push(Item::Note { agent, text, color });
            }
            Kind::Join => self.items.push(Item::Note { agent, text: "joined the thread".into(), color: theme::MUTED }),
            Kind::Leave => {
                self.busy.remove(&agent);
                self.items.push(Item::Note { agent, text: "left the thread".into(), color: theme::MUTED });
            }
            Kind::Error { message } => self.items.push(Item::Note { agent, text: message.clone(), color: theme::DANGER }),
        }
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
    /// Choice I of the open ask.
    Choice(usize),
}

/// The drawn stream: rows, the clickable ones.
#[derive(Default)]
pub struct Drawn {
    pub rows: Vec<Row>,
    pub hits: HashMap<usize, Hit>,
}

/// Colors of the agents (name → color).
pub type Colors = HashMap<String, Color>;

fn color_of(colors: &Colors, name: &str) -> Color {
    colors.get(name).copied().unwrap_or(theme::ACCENT)
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
        Span::styled(format!(" {name} "), Style::default().bg(color).fg(theme::ON_ACCENT).add_modifier(Modifier::BOLD)),
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

pub fn draw(t: &Thread, width: u16, colors: &Colors, roster: &[(String, String)], open: &HashSet<usize>, all: bool, ask_at: usize, picks: &HashSet<usize>, answered: &[String]) -> Drawn {
    let w = usize::from(width.max(10));
    let mut d = Drawn::default();
    let blank = |d: &mut Drawn| d.rows.push(Row::plain(Line::raw("")));
    let mut last_agent: Option<String> = None;
    for item in &t.items {
        match item {
            Item::Prompt { from, to, text, ts } if from == "owner" => {
                // the owner's message: a bubble on the right, solid hover bg, the time on its left
                blank(&mut d);
                let to_note = if to.len() > 1 || last_agent.as_deref() != to.first().map(String::as_str) { format!(" → {}", to.join(", ")) } else { String::new() };
                let (body, files) = split_attachments(text);
                let bw = (w * 3 / 4).max(20).min(w);
                let bg = Style::default().bg(theme::HOVER);
                let mut rows = render::plain(body.trim_end(), bw.saturating_sub(4), theme::text().bg(theme::HOVER), &[]);
                rows.extend(files.iter().map(|f| Row::plain(Line::styled(format!("📎 {f}"), theme::muted().bg(theme::HOVER)))));
                let inner = rows.iter().map(|r| r.line.width()).max().unwrap_or(0);
                let box_w = inner + 4;
                let left = w.saturating_sub(box_w);
                let stamp = format!("{}{to_note} ", clock(*ts));
                for (i, r) in rows.into_iter().enumerate() {
                    let pad = inner - r.line.width();
                    let lead = if i == 0 && stamp.chars().count() < left {
                        vec![Span::styled(format!("{stamp:>left$}"), theme::muted())]
                    } else {
                        vec![Span::raw(" ".repeat(left))]
                    };
                    let mut spans = lead;
                    spans.push(Span::styled("  ", bg));
                    spans.extend(r.line.spans);
                    spans.push(Span::styled(" ".repeat(pad + 2), bg));
                    let shift = (left + 2) as u16;
                    d.rows.push(Row {
                        line: Line::from(spans),
                        links: r.links.into_iter().map(|(a, b, u)| (a + shift, b + shift, u)).collect(),
                        copies: r.copies.into_iter().map(|(a, b, u)| (a + shift, b + shift, u)).collect(),
                    });
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
            Item::Text { agent, text, ts } => {
                let c = color_of(colors, agent);
                if last_agent.as_deref() != Some(agent) {
                    blank(&mut d);
                    d.rows.push(Row::plain(Line::from(badge(agent, c, *ts))));
                } else {
                    blank(&mut d);
                }
                let bar = [Span::styled("│ ", Style::default().fg(c))];
                d.rows.extend(render::markdown(text, w, &bar));
                if let Some((h, what)) = needs_harness(text) {
                    let label = crate::engine::harnesses::get(&h).map_or(h.as_str(), |x| x.label);
                    let what = if what.is_empty() { String::new() } else { format!(" for {what}") };
                    let cap = agents_on(roster, &h, true, agent);
                    let pre = [Span::styled("● ", Style::default().fg(theme::WARN))];
                    d.rows.extend(render::plain(&format!("{agent} needs a {label} agent{what}."), w, Style::default().fg(theme::WARN), &pre));
                    let list = if cap.is_empty() { "none".to_string() } else { cap.iter().map(|(n, _)| format!("@{n}")).collect::<Vec<_>>().join(", ") + " · @agent → + adds it" };
                    d.rows.extend(render::plain(&format!("Agents on {label}: {list}"), w, theme::muted(), &[Span::raw("  ")]));
                }
                last_agent = Some(agent.clone());
            }
            Item::Run { agent, key, calls, start, end } => {
                let c = color_of(colors, agent);
                if last_agent.as_deref() != Some(agent) {
                    blank(&mut d);
                    d.rows.push(Row::plain(Line::from(badge(agent, c, *start))));
                    last_agent = Some(agent.clone());
                }
                let shown = all || open.contains(key);
                let running = calls.iter().any(|c| c.output.is_none()) && t.busy.contains(agent);
                let mark = if shown { "▾ " } else if running { "◦ " } else { "▸ " };
                let failed = calls.iter().filter(|c| matches!(c.output, Some((false, _)))).count();
                let mut summary = run_summary(calls, end.saturating_sub(*start) as f64 / 1000.0);
                if failed > 0 {
                    summary.push_str(&format!(" · {failed} failed"));
                }
                let row_at = d.rows.len();
                let pre = [Span::styled("│ ", Style::default().fg(c)), Span::styled(mark, theme::accent())];
                let rest = [Span::styled("│   ", Style::default().fg(c))];
                for (i, r) in render::plain(&summary, w, theme::muted(), &pre).into_iter().enumerate() {
                    let r = if i == 0 { r } else { Row { line: Line::from([rest.to_vec(), r.line.spans[2..].to_vec()].concat()), ..r } };
                    d.hits.insert(row_at + i, Hit::Run(*key));
                    d.rows.push(r);
                }
                if shown {
                    for call in calls {
                        let head = format!("{} {}", tool_label(&call.name), call_summary(&call.name, &call.input));
                        let bar = [Span::styled("│   ", Style::default().fg(c))];
                        d.rows.extend(render::plain(&head, w, theme::text(), &bar));
                        if call.name == "Bash"
                            && let Some(cmd) = call.input["command"].as_str()
                        {
                            d.rows.extend(render::plain(&format!("$ {cmd}"), w, Style::default().fg(Color::Indexed(110)), &[Span::styled("│     ", Style::default().fg(c))]));
                        }
                        if let Some((ok, out)) = &call.output {
                            let lines: Vec<&str> = out.lines().collect();
                            let tail = lines.len().saturating_sub(12);
                            let mut text = lines[tail..].join("\n");
                            if tail > 0 {
                                text = format!("… {tail} lines\n{text}");
                            }
                            let st = if *ok { theme::muted() } else { Style::default().fg(theme::DANGER) };
                            d.rows.extend(render::plain(&text, w, st, &[Span::styled("│     ", Style::default().fg(c))]));
                        }
                    }
                }
            }
            Item::Note { agent, text, color } => {
                let c = color_of(colors, agent);
                let pre = [Span::styled("● ", Style::default().fg(*color)), Span::styled(format!("{agent} "), Style::default().fg(c))];
                d.rows.extend(render::plain(text, w, Style::default().fg(*color), &pre));
                if *color == theme::DANGER {
                    for h in error_hint(agent, text, roster) {
                        d.rows.extend(render::plain(&h, w, theme::muted(), &[Span::raw("  ")]));
                    }
                }
                last_agent = None;
            }
        }
    }
    // the 1st open ask: below everything, its choices clickable
    if let Some(a) = t.asks.first() {
        let c = color_of(colors, &a.agent);
        blank(&mut d);
        let qs = a.questions();
        let edge = Style::default().fg(theme::WARN);
        let bar = [Span::styled("┃ ", edge)];
        let more = if t.asks.len() > 1 { format!(" · {} more waiting", t.asks.len() - 1) } else { String::new() };
        let what = if qs.is_empty() { "asks to run" } else { "asks" };
        d.rows.push(Row::plain(Line::from(vec![Span::styled("┃ ", edge), Span::styled(a.agent.clone(), Style::default().fg(c).add_modifier(Modifier::BOLD)), Span::styled(format!(" {what}{more}"), edge)])));
        let choices: Vec<(String, String)> = match qs.get(answered.len()) {
            Some((q, header, opts, multi)) => {
                if !header.is_empty() {
                    d.rows.extend(render::plain(header, w, theme::muted(), &bar));
                }
                d.rows.extend(render::plain(q, w, theme::bold(), &bar));
                if *multi {
                    d.rows.extend(render::plain("space: pick · enter: answer", w, theme::muted(), &bar));
                }
                opts.clone()
            }
            None => {
                d.rows.extend(render::plain(&a.summary(), w, theme::bold(), &bar));
                if let Some(cmd) = a.input["command"].as_str() {
                    d.rows.extend(render::plain(&format!("$ {cmd}"), w, Style::default().fg(Color::Indexed(110)), &bar));
                }
                if let Some(plan) = a.input["plan"].as_str() {
                    d.rows.extend(render::markdown(plan, w, &bar));
                }
                let own = a.options();
                if own.is_empty() { vec![("allow".into(), String::new()), ("deny".into(), String::new())] } else { own.into_iter().map(|(_, n, _)| (n, String::new())).collect() }
            }
        };
        for (i, (label, desc)) in choices.iter().enumerate() {
            let cur = i == ask_at;
            let picked = picks.contains(&i);
            let mark = if picked { "◉ " } else if cur { "› " } else { "  " };
            let st = if cur { theme::accent_bold() } else { theme::text() };
            let line = format!("{}. {label}", i + 1);
            let at = d.rows.len();
            let pre = [bar[0].clone(), Span::styled(mark, theme::accent())];
            let mut rows = render::plain(&line, w, st, &pre);
            if !desc.is_empty() {
                rows.extend(render::plain(&format!("     {desc}"), w, theme::muted(), &bar));
            }
            for k in 0..rows.len() {
                d.hits.insert(at + k, Hit::Choice(i));
            }
            d.rows.extend(rows);
        }
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

    fn ev(agent: Option<&str>, kind: Kind) -> Event {
        Event { ts: 1_000_000, agent: agent.map(String::from), kind }
    }

    fn text(d: &Drawn) -> String {
        d.rows.iter().map(|r| r.line.spans.iter().map(|s| s.content.as_ref()).collect::<String>().trim_end().to_string() + "\n").collect()
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
        let d = draw(&t, 60, &Colors::new(), &[], &HashSet::new(), false, 1, &HashSet::new(), &[]);
        let s = text(&d);
        assert!(s.contains("  fix it"), "{s}");
        assert!(s.contains("📎 /tmp/a.png"), "{s}");
        assert!(s.contains("Bash: list · Read: x.rs · 2 calls"), "{s}");
        assert!(s.contains("│ done now"), "{s}");
        assert!(s.contains("┃ › 2. blue") && s.contains("┃   1. red"), "{s}");
        assert!(d.hits.values().any(|h| *h == Hit::Run(1)));
        assert!(widest(&d) <= 60);
        let open: HashSet<usize> = [1].into();
        let s = text(&draw(&t, 60, &Colors::new(), &[], &open, false, 0, &HashSet::new(), &[]));
        assert!(s.contains("$ ls") && s.contains("│     b"), "{s}");
        t.push(&ev(Some("ada"), Kind::Answer { id: "q".into(), allow: true, label: "blue".into() }));
        t.push(&ev(Some("ada"), Kind::TurnEnd { cost: 0.1, input: 1, output: 1, cache_read: 0, cache_write: 0, ctx: 0, ms: 1, error: None }));
        assert!(t.asks.is_empty() && t.busy.is_empty());
        assert!(text(&draw(&t, 60, &Colors::new(), &[], &HashSet::new(), false, 0, &HashSet::new(), &[])).contains("answered: blue"));
        assert_eq!(t.sent().len(), 1);
    }
}
