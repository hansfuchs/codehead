//! Live harness watches, independent of the agent's foreground turn state.
use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};

use ratatui::{Frame, layout::{Rect, Position}, text::{Line, Span}, widgets::{Block, BorderType, Borders, Paragraph, Clear}};
use ratatui::crossterm::event::{KeyCode, KeyEvent, MouseEvent, MouseEventKind, MouseButton};
use serde::{Serialize, Deserialize};
use serde_json::Value;
use std::process::{Command, Stdio};
use crate::{background::Reads, theme};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Watch {
    pub id: String,
    pub agent: String,
    pub harness: String,
    pub description: String,
    pub command: String,
    pub expires: Option<u64>,
    session: String,
}

#[derive(Default)]
pub struct Monitors {
    pub watches: Vec<Watch>,
    reads: Reads<String, Vec<Watch>>,
    open: bool,
    selected: usize,
    scroll: usize,
    bar: Rect,
    popup: Rect,
    hover: bool,
    /// Watches ended here (`d`): hidden until their transcript says so too.
    ended: HashSet<String>,
    /// A line for the view's toast (`d`: what ended, or why not).
    note: Option<String>,
}

impl Monitors {
    pub fn new(thread: &str) -> Self {
        Self { watches: crate::background::snapshot(&format!("monitors-{thread}")).unwrap_or_default(), ..Default::default() }
    }
    pub fn tick(&mut self, thread: &str) -> bool {
        let id = thread.to_string();
        self.reads.request(id.clone(), move || load(&id));
        let mut changed = false;
        for (_, watches) in self.reads.poll() { self.watches = watches; changed = true; }
        self.watches.retain(|w| !self.ended.contains(&w.key()));
        let now = crate::engine::now_ms();
        let n = self.watches.len();
        self.watches.retain(|w| w.expires.is_none_or(|end| end > now));
        changed |= n != self.watches.len();
        self.selected = self.selected.min(self.watches.len().saturating_sub(1));
        if self.watches.is_empty() { self.open = false; }
        changed
    }
    pub fn key(&mut self, k: KeyEvent) -> bool {
        if !self.open { return false; }
        match k.code {
            KeyCode::Esc => self.open = false,
            KeyCode::Up => { self.selected = self.selected.saturating_sub(1); self.scroll = 0; }
            KeyCode::Down => { self.selected = (self.selected + 1).min(self.watches.len().saturating_sub(1)); self.scroll = 0; }
            KeyCode::PageDown => self.scroll += 5,
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(5),
            KeyCode::Char('d') if k.modifiers.is_empty() => self.end_selected(),
            _ => self.open = false,
        }
        true
    }

    /// `d`: end the selected watch (its process), then hide it.
    fn end_selected(&mut self) {
        let Some(w) = self.watches.get(self.selected).cloned() else { return };
        let killed = kill(&w.command);
        self.note = Some(match killed {
            0 => format!("monitor {}: 0 process found (it ended already?) · hidden", w.description),
            n => format!("monitor ended: {} ({n} process{})", w.description, if n == 1 { "" } else { "es" }),
        });
        self.ended.insert(w.key());
        self.watches.retain(|x| x.key() != w.key());
        self.selected = self.selected.min(self.watches.len().saturating_sub(1));
        self.scroll = 0;
        if self.watches.is_empty() { self.open = false; }
    }

    /// The last `d` result, once.
    pub fn take_note(&mut self) -> Option<String> {
        self.note.take()
    }

    /// Rows for the monitors area at WIDTH: 0 (0 watches), 1 (the row), or the row + the open
    /// box (its lines + 2 border rows, at most MAX in all).
    pub fn rows(&self, width: u16, max: u16) -> u16 {
        if self.watches.is_empty() { return 0; }
        if !self.open { return 1; }
        (1 + 2 + self.lines(width.saturating_sub(2).max(1) as usize).len() as u16).min(max.max(1))
    }

    /// The open box's text, wrapped to WIDTH: 1 row per watch, then the selected one's details.
    fn lines(&self, width: usize) -> Vec<Line<'static>> {
        let Some(watch) = self.watches.get(self.selected) else { return Vec::new() };
        let mut out: Vec<Line<'static>> = self.watches.iter().enumerate().map(|(i, w)| {
            let chars: Vec<_> = format!("{} {} · {} · {}", if i == self.selected { "›" } else { " " }, w.agent, w.harness, w.description).chars().collect();
            let (_, end) = crate::textarea::wrap(&chars, width)[0];
            let st = if i == self.selected { theme::accent_bold() } else { theme::text() };
            Line::styled(chars[..end].iter().collect::<String>(), st)
        }).collect();
        let mut text = vec![String::new(), format!("watch: {}", watch.description), format!("task: {}", watch.id), format!("method: {}", watch.command)];
        if let Some(expires) = watch.expires { text.push(format!("expires in: {}s", expires.saturating_sub(crate::engine::now_ms()) / 1000)); }
        for t in text {
            let chars: Vec<_> = t.chars().map(|c| if c.is_control() && c != '\n' { ' ' } else { c }).collect();
            for line in chars.split(|&c| c == '\n') {
                for (a, b) in crate::textarea::wrap(line, width) {
                    out.push(Line::styled(line[a..b].iter().collect::<String>(), theme::muted()));
                }
            }
        }
        out
    }
    pub fn mouse(&mut self, m: MouseEvent) -> bool {
        let at = Position { x: m.column, y: m.row };
        self.hover = self.bar.contains(at);
        if matches!(m.kind, MouseEventKind::Down(MouseButton::Left)) && self.bar.contains(at) {
            self.open = !self.open;
            return true;
        }
        if self.open && self.popup.contains(at) {
            match m.kind {
                MouseEventKind::ScrollDown => self.scroll += 1,
                MouseEventKind::ScrollUp => self.scroll = self.scroll.saturating_sub(1),
                MouseEventKind::Down(MouseButton::Left) => {
                    let row = usize::from(m.row.saturating_sub(self.popup.y + 1)) + self.scroll;
                    if row < self.watches.len() { self.selected = row; self.scroll = 0; }
                }
                _ => {}
            }
            return true;
        }
        false
    }
    /// AREA: the monitors area (`rows`): the open box on top, the monitors row at the bottom.
    pub fn render(&mut self, f: &mut Frame, area: Rect, tick: usize) {
        self.bar = Rect::default(); self.popup = Rect::default();
        if self.watches.is_empty() || area.is_empty() { return; }
        let anchor = Rect { y: area.bottom() - 1, height: 1, ..area };
        self.bar = anchor;
        f.render_widget(Clear, anchor);
        let descriptions = self.watches.iter().map(|w| format!("{}: {}", w.agent, w.description)).collect::<Vec<_>>().join(" · ");
        let wave = ["◐", "◓", "◑", "◒"][tick % 4];
        let style = if self.hover { theme::accent().bg(theme::pal().hover) } else { theme::accent() };
        let hint = if self.open { "click: close" } else { "click: details" };
        f.render_widget(Paragraph::new(format!("{wave} monitors ({}) · {descriptions} · {hint}", self.watches.len())).style(style), anchor);
        if !self.open || area.height < 4 { return; }
        let r = Rect { height: area.height - 1, ..area };
        self.popup = r;
        f.render_widget(Clear, r);
        let block = Block::new().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(theme::accent())
            .title(Span::styled(" monitors ", theme::accent_bold()))
            .title_bottom(Line::styled(" ↑↓ select · d end · pgup/pgdn scroll · esc close ", theme::muted()).right_aligned());
        let inner = block.inner(r);
        f.render_widget(block, r);
        let inner = Rect { x: inner.x + 1, width: inner.width.saturating_sub(2), ..inner };
        let lines = self.lines(inner.width.max(1) as usize);
        self.scroll = self.scroll.min(lines.len().saturating_sub(inner.height as usize));
        f.render_widget(Paragraph::new(lines).scroll((self.scroll as u16, 0)).style(theme::text()), inner);
    }
}

impl Watch {
    /// Session + task id: 1 watch.
    fn key(&self) -> String {
        format!("{}:{}", self.session, self.id)
    }
}

/// End the processes running COMMAND (their command line holds its 1st line): each one with its
/// children, SIGTERM. Never codehead itself. The number ended.
fn kill(command: &str) -> usize {
    let needle = command.lines().map(str::trim).find(|l| l.len() >= 6).unwrap_or_default();
    if needle.is_empty() { return 0; }
    let Ok(out) = Command::new("ps").args(["-axwwo", "pid=,command="]).stdin(Stdio::null()).output() else { return 0 };
    let me = std::process::id();
    let pids: Vec<i32> = String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| {
        let (pid, cmd) = l.trim_start().split_once(' ')?;
        let pid: u32 = pid.parse().ok()?;
        // the shell that runs the command, not a `ps` / `grep` that only names it
        (pid != me && cmd.contains(needle) && !cmd.starts_with("ps ")).then_some(pid as i32)
    }).collect();
    for &pid in &pids {
        // children first listed, the shell ended first (else it goes on to its next command)
        let children: Vec<i32> = Command::new("pgrep").args(["-P", &pid.to_string()]).stdin(Stdio::null()).output()
            .map(|o| String::from_utf8_lossy(&o.stdout).lines().filter_map(|l| l.trim().parse().ok()).collect()).unwrap_or_default();
        unsafe { libc::kill(pid, libc::SIGTERM) };
        for c in children { unsafe { libc::kill(c, libc::SIGTERM) }; }
    }
    pids.len()
}

fn task_id(output: &Value) -> Option<String> {
    if let Some(id) = output.get("task_id").or_else(|| output.get("taskId")).and_then(Value::as_str) { return Some(id.to_string()); }
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| regex::Regex::new(r"(?:task |<task_id>|background with ID: )([A-Za-z0-9_-]+)").unwrap());
    re.captures(output.as_str()?).map(|c| c[1].to_string())
}

fn parse(rows: &[Value], now: u64) -> Vec<Watch> {
    let mut watches = HashMap::new();
    for row in rows {
        let c = &row["content"];
        let input = &c["raw_input"];
        let name = c["title"].as_str().unwrap_or_default();
        let session = row["session"].as_str().unwrap_or_default();
        if name == "TaskStop" {
            if let Some(id) = input["task_id"].as_str() { watches.remove(&format!("{session}:{id}")); }
            continue;
        }
        let codex = input["type"] == "commandExecution";
        let monitor = name.eq_ignore_ascii_case("monitor") || name.to_ascii_lowercase().ends_with("__monitor");
        let background = input["command"].is_string() && input["run_in_background"] == true;
        if !monitor && !background && !codex { continue; }
        let Some(id) = (if codex { input["processId"].as_str().map(str::to_string) } else { task_id(&c["raw_output"]) }) else { continue };
        let key = format!("{session}:{id}");
        let status = c["raw_output"]["status"].as_str().or(row["status"].as_str()).or(input["status"].as_str()).unwrap_or_default();
        if codex && !["inProgress", "in_progress", "running"].contains(&status) { watches.remove(&key); continue; }
        let timeout = input["timeout_ms"].as_u64().or(if monitor { Some(1_800_000) } else { None });
        let expires = timeout.map(|t| row["at"].as_u64().unwrap_or(now).saturating_add(t));
        if expires.is_some_and(|e| e <= now) { continue; }
        let command = input["command"].as_str().unwrap_or_default().to_string();
        watches.insert(key, Watch { id, agent: row["agent"].as_str().unwrap_or("agent").into(), harness: row["harness"].as_str().unwrap_or("unknown").into(),
            description: input["description"].as_str().unwrap_or_else(|| command.lines().next().unwrap_or("background task")).chars().map(|c| if c.is_control() { ' ' } else { c }).collect(), command, expires, session: session.into() });
    }
    let mut watches: Vec<_> = watches.into_values().collect();
    watches.sort_by(|a,b| (&a.agent,&a.id).cmp(&(&b.agent,&b.id)));
    watches
}

#[derive(Default)]
struct Transcript { offset: u64, tail: String, ended: HashSet<String> }

fn completed(path: &str) -> HashSet<String> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<HashMap<String, Transcript>>> = std::sync::OnceLock::new();
    let Ok(mut cache) = CACHE.get_or_init(Default::default).lock() else { return HashSet::new() };
    let Ok(mut file) = std::fs::File::open(path) else { return HashSet::new() };
    let entry = cache.entry(path.into()).or_default();
    if file.metadata().is_ok_and(|m| m.len() < entry.offset) { *entry = Transcript::default(); }
    let _ = file.seek(SeekFrom::Start(entry.offset));
    let mut bytes = Vec::new();
    if file.read_to_end(&mut bytes).is_err() { return entry.ended.clone(); }
    entry.offset += bytes.len() as u64;
    entry.tail.push_str(&String::from_utf8_lossy(&bytes));
    if let Some(end) = entry.tail.rfind('\n') {
        for line in entry.tail[..=end].lines() {
            let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
            let text = if v["type"] == "queue-operation" { v["content"].as_str() } else if v["type"] == "attachment" { v["attachment"]["prompt"].as_str() } else { None };
            let Some(text) = text.filter(|t| t.contains("<task-notification>")) else { continue };
            if ["completed", "failed", "killed", "cancelled", "stopped"].iter().any(|status| text.contains(&format!("<status>{status}</status>")))
                && let Some(id) = text.split_once("<task-id>").and_then(|(_, t)| t.split_once("</task-id>")).map(|(id,_)| id)
            { entry.ended.insert(id.into()); }
        }
        entry.tail.drain(..=end);
    }
    entry.ended.clone()
}

/// Watches of THREAD: its agents' Monitor calls and background commands, from the event log,
/// minus the tasks their Claude transcripts report ended.
fn load(thread: &str) -> Vec<Watch> {
    use crate::engine::store::{self, Kind};
    let Some(t) = store::load(thread) else { return Vec::new() };
    let (live, _) = crate::engine::client::state(&t.id);
    let mut rows: Vec<Value> = Vec::new();
    let mut at: HashMap<String, usize> = HashMap::new();
    for e in store::events(&t.id) {
        let Some(agent) = e.agent.clone().filter(|a| live.contains(a)) else { continue };
        match e.kind {
            Kind::Tool { id, name, input } => {
                let watch = name.eq_ignore_ascii_case("monitor") || name.ends_with("__monitor") || name == "TaskStop" || input["run_in_background"] == true;
                if watch {
                    at.insert(id, rows.len());
                    rows.push(serde_json::json!({"session": agent, "agent": agent, "harness": "claude", "at": e.ts, "content": {"title": name, "raw_input": input}}));
                }
            }
            Kind::ToolResult { id, output, .. } => {
                if let Some(&i) = at.get(&id) {
                    rows[i]["content"]["raw_output"] = Value::String(output);
                }
            }
            _ => {}
        }
    }
    let mut watches = parse(&rows, crate::engine::now_ms());
    let root = crate::config::home().join(".claude/projects");
    let transcript = |agent: &str| -> Option<String> {
        let sid = t.sessions.get(agent)?;
        std::fs::read_dir(&root).ok()?.flatten().map(|d| d.path().join(format!("{sid}.jsonl"))).find(|p| p.is_file()).map(|p| p.display().to_string())
    };
    let ended: HashMap<String, HashSet<String>> = watches.iter().map(|w| w.session.clone()).collect::<HashSet<_>>().into_iter().filter_map(|a| Some((a.clone(), completed(&transcript(&a)?)))).collect();
    watches.retain(|w| !ended.get(&w.session).is_some_and(|ids| ids.contains(&w.id)));
    watches
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn monitor(id: &str) -> Value {
        json!({"session":"session","agent":"amon","harness":"claude","at":1000,"content":{
            "title":"Monitor","raw_input":{"command":"tail -f smoke.log | grep result","description":"smoke job progress","timeout_ms":30000},
            "raw_output":format!("Monitor started (task {id}, expires in 30m)")}})
    }

    #[test]
    fn monitors_survive_idle_registration_then_expire_or_stop() {
        let row = monitor("watch1");
        let watches = parse(std::slice::from_ref(&row), 2000);
        assert_eq!(watches.len(), 1);
        assert_eq!(watches[0].description, "smoke job progress");
        assert_eq!(watches[0].command, "tail -f smoke.log | grep result");
        assert!(parse(std::slice::from_ref(&row), 31000).is_empty());
        let stop = json!({"session":"session","content":{"title":"TaskStop","raw_input":{"task_id":"watch1"}}});
        assert!(parse(&[row, stop], 2000).is_empty());
        assert!(parse(&[json!({"content":{"title":"ToolSearch","raw_input":{"query":"select:Monitor"}}})], 2000).is_empty());
    }

    #[test]
    fn codex_process_completion_overrides_stale_input_status() {
        let row = json!({"session":"codex","agent":"paimon","harness":"codex","status":"in_progress","content":{
            "title":"run tests","raw_input":{"type":"commandExecution","processId":"123","status":"inProgress","command":"cargo test"}}});
        assert_eq!(parse(std::slice::from_ref(&row), 2000).len(), 1);
        let mut done = row.clone(); done["status"] = json!("completed");
        assert!(parse(&[row, done], 2000).is_empty());
    }

    #[test]
    fn claude_background_tasks_register_by_structured_task_id() {
        let row = json!({"session":"claude","content":{"title":"Bash","raw_input":{"command":"run smoke","run_in_background":true},"raw_output":{"task_id":"job1"}}});
        let watches = parse(&[row], 2000);
        assert_eq!(watches.len(), 1);
        assert_eq!(watches[0].id, "job1");
        assert!(watches[0].expires.is_none());
        let row = json!({"session":"claude","content":{"title":"`run smoke`","raw_input":{"command":"run smoke","run_in_background":true},"raw_output":"Command running in background with ID: bg-job. Output is being written to: /tmp/job"}});
        assert_eq!(parse(&[row], 2000)[0].id, "bg-job");
    }

    #[test]
    fn task_notifications_end_only_the_named_task_and_support_incremental_reads() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("codehead-watch-{}-{}", std::process::id(), (crate::engine::now_ms() / 1000)));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("transcript.jsonl");
        let mut file = std::fs::File::create(&path).unwrap();
        writeln!(file, "{}", json!({"type":"assistant","content":"<task-notification><task-id>not-real</task-id><status>completed</status></task-notification>"})).unwrap();
        assert!(completed(path.to_str().unwrap()).is_empty());
        writeln!(file, "{}", json!({"type":"queue-operation","content":"<task-notification><task-id>job1</task-id><status>running</status></task-notification>"})).unwrap();
        assert!(completed(path.to_str().unwrap()).is_empty());
        writeln!(file, "{}", json!({"type":"queue-operation","content":"<task-notification><task-id>job1</task-id><status>completed</status></task-notification>"})).unwrap();
        assert_eq!(completed(path.to_str().unwrap()), HashSet::from(["job1".into()]));
        assert_eq!(completed(path.to_str().unwrap()), HashSet::from(["job1".into()]));
        drop(file);
        std::fs::remove_file(path).unwrap(); std::fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn monitor_bar_click_opens_details_and_escape_closes_without_navigation() {
        use ratatui::crossterm::event::KeyModifiers;
        let mut panel = Monitors { watches: parse(&[monitor("watch1")], 2000), ..Default::default() };
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
        assert_eq!(panel.rows(100, 20), 1);
        term.draw(|f| panel.render(f, Rect::new(0, 24, 100, 1), 0)).unwrap();
        assert!(panel.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: 1, row: 24, modifiers: KeyModifiers::NONE }));
        // open: the box takes rows above the monitors row (the view's layout gives them)
        let rows = panel.rows(100, 20);
        assert!(rows > 4, "{rows}");
        let area = Rect::new(0, 25 - rows, 100, rows);
        term.draw(|f| panel.render(f, area, 1)).unwrap();
        assert!(!panel.popup.is_empty() && panel.popup.bottom() == 24);
        let buffer = term.backend().buffer();
        let text = buffer.content.iter().map(|c| c.symbol()).collect::<String>();
        assert!(text.contains("smoke job progress"));
        assert!(text.contains("tail -f smoke.log"));
        assert!(text.contains("d end"));
        assert!(panel.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
        assert!(!panel.open);
        assert_eq!(panel.rows(100, 20), 1);
    }

    #[test]
    fn d_ends_the_selected_watch_and_its_process() {
        use ratatui::crossterm::event::KeyModifiers;
        // a real process whose command line holds the watch command
        let tag = format!("codehead-monitor-end-{}-{}", std::process::id(), (crate::engine::now_ms() / 1000));
        // `; echo`: sh keeps running (0 exec of the last command), like a harness shell
        let mut child = Command::new("sh").args(["-c", &format!("sleep 30; echo {tag}")]).spawn().unwrap();
        let mut watch = monitor("watch1");
        watch["content"]["raw_input"]["command"] = json!(format!("sleep 30; echo {tag}"));
        let mut panel = Monitors { watches: parse(&[watch], 2000), open: true, ..Default::default() };
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(panel.key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE)));
        assert!(panel.watches.is_empty() && !panel.open);
        assert!(panel.take_note().unwrap().starts_with("monitor ended"));
        let status = child.wait().unwrap();
        assert!(!status.success(), "SIGTERM ended it");
        // a later read: still hidden
        panel.watches = parse(&[monitor("watch1")], 2000);
        panel.watches.retain(|w| !panel.ended.contains(&w.key()));
        assert!(panel.watches.is_empty());
    }
}
