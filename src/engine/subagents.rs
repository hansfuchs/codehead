//! Subagents of the agents on a thread, host side. A subagent starts with its master's task
//! tool call (Claude Code `Agent` / `Task`, an ACP agent's task tool). Claude Code keeps a file per
//! subagent next to the session (`<session>/subagents/agent-<id>.jsonl` + `.meta.json`): its
//! steps, nested subagents, the end of a background subagent and its report come from there.
//! The tracker turns all of it into `sub_start` / `sub_info` / `sub_end` events of the log.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::store::{Event, Kind};

/// A session's `subagents` dir: found again after this (a resume in another dir moves it).
const REDIR: std::time::Duration = std::time::Duration::from_secs(60);

/// A task tool: its call starts a subagent.
pub fn is_sub_tool(name: &str) -> bool {
    matches!(name, "Agent" | "Task") || matches!(name.to_ascii_lowercase().as_str(), "task" | "subagent" | "spawn_agent" | "delegate_task" | "dispatch_agent")
}

/// A running subagent.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Run {
    /// Its master: the codehead agent.
    pub agent: String,
    pub description: String,
    pub background: bool,
    /// Started by another subagent (it ends with its meta file, not a tool result of the master).
    pub nested: bool,
    /// Claude Code: its transcript, its agent id (the task id of TaskStop).
    pub transcript: Option<PathBuf>,
    pub task: String,
    /// Resumed at (unix ms; 0: never): a `finished` meta file older than this is its last end.
    pub resumed: u64,
}

/// An ended subagent whose files changed this long (ms) after its end: resumed.
const RESUME_GAP: u64 = 2_000;

/// A resumed subagent: its meta file stays `finished` and its transcript unchanged this long
/// (ms): the resume never ran (it ends again).
const RESUME_WAIT: u64 = 30_000;

/// Ended subagents this old (ms) count as done for good: 0 scans for a resume.
const RESUMABLE: u64 = 24 * 60 * 60 * 1000;

/// Events to log: (master, kind).
pub type Logs = Vec<(String, Kind)>;

#[derive(Default)]
pub struct Tracker {
    /// By id (the tool call that started it).
    pub running: HashMap<String, Run>,
    /// Ended ids: a later scan never starts them again.
    ended: HashSet<String>,
    /// Ended ones, by id: (the run, its end in unix ms). A message of the master resumes them.
    done: HashMap<String, (Run, u64)>,
    /// Claude Code session id → its `subagents` dir, when found (found again after `REDIR`).
    dirs: HashMap<String, (PathBuf, std::time::Instant)>,
    /// Meta files older than this (ms) belong to earlier processes.
    since: u64,
}

fn cut(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n…", &s[..end])
}

/// What a subagent does, from its tool input: the description, else the title, else the 1st line of the prompt.
fn description_of(input: &Value) -> String {
    let p = input["prompt"].as_str().unwrap_or_default();
    let d = input["description"].as_str().or_else(|| input["title"].as_str()).filter(|s| !s.trim().is_empty()).unwrap_or_else(|| p.lines().next().unwrap_or("subagent"));
    d.chars().map(|c| if c.is_control() { ' ' } else { c }).take(120).collect()
}

/// `agentId: <id>` in a tool result (Claude Code's background launch).
fn agent_id_in(output: &str) -> Option<String> {
    let at = output.find("agentId:")? + "agentId:".len();
    let id: String = output[at..].trim_start().chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
    (!id.is_empty()).then_some(id)
}

/// OUTPUT is the launch of a background subagent (Claude Code: `Async agent launched
/// successfully … agentId: …`), not its report.
fn async_launch(output: &str) -> bool {
    let head: String = output.chars().take(400).collect();
    head.contains("Async agent launched") || (agent_id_in(&head).is_some() && head.contains("in the background"))
}

/// The tail of FILE (MAX bytes, a cut 1st line left out), parsed lines.
fn tail_lines(file: &Path, max: u64) -> Vec<Value> {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(file) else { return Vec::new() };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let from = len.saturating_sub(max);
    let _ = f.seek(SeekFrom::Start(from));
    let mut buf = Vec::new();
    let _ = f.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf);
    let body = if from > 0 { text.split_once('\n').map_or("", |x| x.1) } else { &text };
    body.lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

/// The report of a subagent: its `SubagentHandback` message, else its last text. From TRANSCRIPT.
pub fn result_of(transcript: &Path) -> Option<String> {
    for v in tail_lines(transcript, 512 << 10).iter().rev() {
        if v["type"] != "assistant" {
            continue;
        }
        let blocks = v["message"]["content"].as_array().cloned().unwrap_or_default();
        if let Some(m) = blocks.iter().find(|b| b["type"] == "tool_use" && b["name"] == "SubagentHandback").and_then(|b| b["input"]["message"].as_str()) {
            return Some(cut(m, 16 << 10));
        }
        let text: Vec<&str> = blocks.iter().filter(|b| b["type"] == "text").filter_map(|b| b["text"].as_str()).filter(|t| !t.trim().is_empty()).collect();
        if !text.is_empty() {
            return Some(cut(&text.join("\n\n"), 16 << 10));
        }
    }
    None
}

/// What the subagent does now, from the end of its TRANSCRIPT: its last tool call (`Edit src/a.rs`)
/// or the 1st line of its last text.
pub fn activity_of(transcript: &Path) -> Option<String> {
    for v in tail_lines(transcript, 64 << 10).iter().rev() {
        if v["type"] != "assistant" {
            continue;
        }
        for b in v["message"]["content"].as_array().into_iter().flatten().rev() {
            match b["type"].as_str() {
                Some("tool_use") => return Some(crate::handoff::tool_line(b["name"].as_str().unwrap_or_default(), &b["input"])),
                Some("text") => {
                    if let Some(l) = b["text"].as_str().and_then(|t| t.lines().find(|l| !l.trim().is_empty())) {
                        return Some(l.trim().chars().take(160).collect());
                    }
                }
                _ => {}
            }
        }
    }
    None
}

/// The 1st user message of TRANSCRIPT: the subagent's task.
fn prompt_of(transcript: &Path) -> String {
    use std::io::{BufRead, BufReader};
    let Ok(f) = std::fs::File::open(transcript) else { return String::new() };
    for l in BufReader::new(f).lines().take(20).map_while(Result::ok) {
        let Ok(v) = serde_json::from_str::<Value>(&l) else { continue };
        if v["type"] == "user" {
            let c = &v["message"]["content"];
            let t = c.as_str().map(String::from).unwrap_or_else(|| c.as_array().into_iter().flatten().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n"));
            return cut(&t, 2048);
        }
    }
    String::new()
}

fn mtime_ms(p: &Path) -> u64 {
    std::fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as u64)
}

impl Tracker {
    /// The tracker of a host that starts now, from the thread's log: subagents the log has open
    /// ran in a process that is gone: they end as lost.
    pub fn from_log(events: &[Event]) -> (Tracker, Logs) {
        let mut t = Tracker { since: super::now_ms(), ..Default::default() };
        let mut open: Vec<(String, String)> = Vec::new();
        let mut runs: HashMap<String, Run> = HashMap::new();
        for e in events {
            let agent = e.agent.clone().unwrap_or_default();
            match &e.kind {
                Kind::SubStart { id, parent, description, background, .. } => {
                    open.push((agent.clone(), id.clone()));
                    runs.insert(id.clone(), Run { agent, description: description.clone(), background: *background, nested: parent.is_some(), ..Default::default() });
                }
                Kind::SubInfo { id, transcript, task } => {
                    if let Some(r) = runs.get_mut(id) {
                        if !transcript.is_empty() {
                            r.transcript = Some(PathBuf::from(transcript));
                        }
                        if !task.is_empty() {
                            r.task = task.clone();
                        }
                    }
                }
                Kind::SubEnd { id, .. } => {
                    t.ended.insert(id.clone());
                    open.retain(|(_, x)| x != id);
                    if let Some(r) = runs.get(id) {
                        t.done.insert(id.clone(), (r.clone(), e.ts));
                    }
                }
                Kind::SubResume { id } => {
                    t.ended.remove(id);
                    t.done.remove(id);
                    if !open.iter().any(|(_, x)| x == id) {
                        open.push((agent, id.clone()));
                    }
                }
                _ => {}
            }
        }
        let logs = open
            .into_iter()
            .map(|(a, id)| {
                t.ended.insert(id.clone());
                if let Some(r) = runs.remove(&id) {
                    t.done.insert(id.clone(), (r, t.since));
                }
                (a, Kind::SubEnd { id, state: "lost".into(), result: "codehead's thread host restarted: the harness process that ran it is gone".into() })
            })
            .collect();
        (t, logs)
    }

    /// Subagents run.
    pub fn any(&self) -> bool {
        !self.running.is_empty()
    }

    /// Masters with running subagents.
    pub fn masters(&self) -> HashSet<String> {
        self.running.values().map(|r| r.agent.clone()).collect()
    }

    /// Ended subagents with files (Claude Code), ended in the last RESUMABLE: a message of their
    /// master can resume them.
    pub fn resumable(&self) -> bool {
        let since = super::now_ms().saturating_sub(RESUMABLE);
        self.done.values().any(|(r, end)| r.transcript.is_some() && *end >= since)
    }

    /// ID ends: out of the running ones, kept with its end time for a resume.
    fn end(&mut self, id: &str) -> Option<Run> {
        let r = self.running.remove(id)?;
        self.ended.insert(id.to_string());
        self.done.insert(id.to_string(), (r.clone(), super::now_ms()));
        Some(r)
    }

    /// Ended subagent ID runs again. It ends with its meta file (0 tool result of its master).
    fn resume(&mut self, id: &str) -> Logs {
        let Some((mut r, _)) = self.done.remove(id) else { return Vec::new() };
        self.ended.remove(id);
        r.resumed = super::now_ms();
        r.background = true;
        let agent = r.agent.clone();
        self.running.insert(id.to_string(), r);
        vec![(agent, Kind::SubResume { id: id.to_string() })]
    }

    /// An event of AGENT: a task tool call starts a subagent; its result ends it (a background
    /// one: its result is the launch, the meta file says when it ends).
    pub fn event(&mut self, agent: &str, kind: &Kind) -> Logs {
        match kind {
            // a task tool, or any tool call with a subagent type (ACP agents name their tools freely)
            Kind::Tool { id, name, input } if (is_sub_tool(name) || input["subagent_type"].is_string()) && !self.running.contains_key(id) && !self.ended.contains(id) => {
                let description = description_of(input);
                let background = input["run_in_background"] == true;
                self.running.insert(id.clone(), Run { agent: agent.into(), description: description.clone(), background, ..Default::default() });
                let prompt = cut(input["prompt"].as_str().unwrap_or_default(), 2048);
                let agent_type = input["subagent_type"].as_str().unwrap_or_default().to_string();
                vec![(agent.into(), Kind::SubStart { id: id.clone(), parent: None, description, agent_type, background, prompt })]
            }
            Kind::ToolResult { id, ok, output } if self.running.get(id).is_some_and(|r| r.agent == agent) => {
                let r = self.running.get_mut(id).expect("checked");
                // Claude Code starts subagents in the background by default (0 `run_in_background`
                // in the input): the result is the launch, not the end
                if *ok && !r.background && async_launch(output) {
                    r.background = true;
                }
                if r.background && *ok {
                    if let Some(task) = agent_id_in(output).filter(|t| *t != r.task) {
                        r.task = task.clone();
                        let transcript = r.transcript.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
                        return vec![(agent.into(), Kind::SubInfo { id: id.clone(), transcript, task })];
                    }
                    return Vec::new();
                }
                let r = self.end(id).expect("checked");
                let result = r.transcript.as_deref().and_then(result_of).filter(|_| *ok).unwrap_or_else(|| output.clone());
                vec![(agent.into(), Kind::SubEnd { id: id.clone(), state: if *ok { "done" } else { "failed" }.into(), result })]
            }
            // a message to an ended subagent (Claude Code `SendMessage`, `to`: its agent id) resumes it
            Kind::Tool { name, input, .. } if name == "SendMessage" => {
                let to = input["to"].as_str().unwrap_or_default();
                let id = self.done.iter().find(|(_, (r, _))| r.agent == agent && !to.is_empty() && r.task == to).map(|(id, _)| id.clone());
                id.map(|id| self.resume(&id)).unwrap_or_default()
            }
            _ => Vec::new(),
        }
    }

    /// The harness process of AGENT ended (WHY): its running subagents are lost.
    pub fn exit(&mut self, agent: &str, why: &str) -> Logs {
        let ids: Vec<String> = self.running.iter().filter(|(_, r)| r.agent == agent).map(|(id, _)| id.clone()).collect();
        ids.into_iter()
            .map(|id| {
                self.end(&id);
                (agent.to_string(), Kind::SubEnd { id, state: "lost".into(), result: why.to_string() })
            })
            .collect()
    }

    /// The owner stops ID: it leaves the tracker (its end is logged by the caller).
    pub fn stop(&mut self, id: &str) -> Option<Run> {
        self.end(id)
    }

    /// The `subagents` dir of Claude Code session SID under ROOT (`~/.claude/projects`).
    /// A session resumed in another dir (the thread moved repo): the newest transcript copy, found
    /// again after REDIR.
    fn dir_of(&mut self, root: &Path, sid: &str) -> Option<PathBuf> {
        if let Some((d, _)) = self.dirs.get(sid).filter(|(_, at)| at.elapsed() < REDIR) {
            return Some(d.clone());
        }
        let file = crate::claude::session_file(root, sid)?;
        let d = file.parent()?.join(sid).join("subagents");
        self.dirs.insert(sid.to_string(), (d.clone(), std::time::Instant::now()));
        Some(d)
    }

    /// Claude Code: the subagent files of SESSIONS ((master, session id)) under ROOT. Gives
    /// transcripts, nested subagents, the ends of background and nested ones.
    pub fn scan(&mut self, root: &Path, sessions: &[(String, String)]) -> Logs {
        let mut logs = Logs::new();
        for (agent, sid) in sessions {
            let Some(dir) = self.dir_of(root, sid) else { continue };
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            let mut metas: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.to_string_lossy().ends_with(".meta.json")).collect();
            metas.sort();
            for meta_path in metas {
                let Ok(meta) = std::fs::read(&meta_path).map_err(|_| ()).and_then(|b| serde_json::from_slice::<Value>(&b).map_err(|_| ())) else { continue };
                let Some(tool) = meta["toolUseId"].as_str().map(String::from) else { continue };
                let name = meta_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let task = name.trim_start_matches("agent-").trim_end_matches(".meta.json").to_string();
                let transcript = dir.join(format!("agent-{task}.jsonl"));
                let finished = meta["finished"] == true;
                if let Some(r) = self.running.get_mut(&tool).filter(|r| r.agent == *agent) {
                    if r.transcript.is_none() {
                        r.transcript = Some(transcript.clone());
                        r.task = task.clone();
                        logs.push((agent.clone(), Kind::SubInfo { id: tool.clone(), transcript: transcript.display().to_string(), task: task.clone() }));
                    }
                    // resumed: a `finished` from before the resume is its last end, unless the
                    // resume never ran (0 change of its files in RESUME_WAIT)
                    let now = super::now_ms();
                    let fresh = r.resumed == 0 || mtime_ms(&meta_path) > r.resumed || (now.saturating_sub(r.resumed) > RESUME_WAIT && mtime_ms(&transcript) <= r.resumed);
                    // a background or nested one ends here; a foreground one with its tool result
                    if finished && fresh && (r.background || r.nested) {
                        self.end(&tool);
                        let result = result_of(&transcript).unwrap_or_else(|| "(0 report)".into());
                        logs.push((agent.clone(), Kind::SubEnd { id: tool, state: "done".into(), result }));
                    }
                    continue;
                }
                // an ended one whose files changed after its end, 0 `finished`: resumed (a
                // message the events missed: of a subagent, before a host restart)
                let changed = |end: u64| mtime_ms(&meta_path) > end + RESUME_GAP && mtime_ms(&transcript) > end + RESUME_GAP;
                if !finished && self.done.get(&tool).is_some_and(|(r, end)| r.agent == *agent && changed(*end)) {
                    logs.extend(self.resume(&tool));
                    continue;
                }
                if finished || self.ended.contains(&tool) || mtime_ms(&transcript).max(mtime_ms(&meta_path)) < self.since {
                    continue;
                }
                // a subagent of a subagent: its parent's transcript holds the call
                let parent = self
                    .running
                    .iter()
                    .filter(|(_, r)| r.agent == *agent)
                    .find(|(_, r)| r.transcript.as_ref().is_some_and(|p| std::fs::read_to_string(p).is_ok_and(|t| t.contains(&tool))))
                    .map(|(id, _)| id.clone());
                let description = meta["description"].as_str().map(String::from).unwrap_or_else(|| "subagent".into());
                let background = meta["requestShape"] == "background";
                self.running.insert(tool.clone(), Run { agent: agent.clone(), description: description.clone(), background, nested: true, transcript: Some(transcript.clone()), task: task.clone(), resumed: 0 });
                let agent_type = meta["agentType"].as_str().unwrap_or_default().to_string();
                logs.push((agent.clone(), Kind::SubStart { id: tool.clone(), parent, description, agent_type, background, prompt: prompt_of(&transcript) }));
                logs.push((agent.clone(), Kind::SubInfo { id: tool, transcript: transcript.display().to_string(), task }));
            }
        }
        logs
    }

    /// The running subagent whose task id is TASK (Claude Code's `agent_id` of a request).
    pub fn by_task(&self, task: &str) -> Option<(&String, &Run)> {
        self.running.iter().find(|(_, r)| !task.is_empty() && r.task == task)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tool(id: &str, input: Value) -> Kind {
        Kind::Tool { id: id.into(), name: "Agent".into(), input }
    }

    #[test]
    fn foreground_start_and_end_by_the_tool_result() {
        let (mut t, lost) = Tracker::from_log(&[]);
        assert!(lost.is_empty());
        let logs = t.event("ada", &tool("t1", json!({"description": "find the bug", "prompt": "look at x", "subagent_type": "Explore"})));
        assert!(matches!(&logs[0].1, Kind::SubStart { id, description, agent_type, background: false, prompt, .. } if id == "t1" && description == "find the bug" && agent_type == "Explore" && prompt == "look at x"));
        assert_eq!(t.masters(), HashSet::from(["ada".to_string()]));
        // another agent's result with the same id: 0 end
        assert!(t.event("bob", &Kind::ToolResult { id: "t1".into(), ok: true, output: "x".into() }).is_empty());
        let logs = t.event("ada", &Kind::ToolResult { id: "t1".into(), ok: true, output: "found it".into() });
        assert!(matches!(&logs[0].1, Kind::SubEnd { state, result, .. } if state == "done" && result == "found it"));
        assert!(!t.any());
        // the same call again (a replay): 0 start
        assert!(t.event("ada", &tool("t1", json!({}))).is_empty());
    }

    #[test]
    fn background_launch_gives_the_task_id_then_the_meta_file_ends_it() {
        let root = std::env::temp_dir().join(format!("codehead-subs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("proj/s1/subagents");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(root.join("proj/s1.jsonl"), "").unwrap();
        let (mut t, _) = Tracker::from_log(&[]);
        t.since = 0;
        t.event("ada", &tool("t2", json!({"description": "long job", "run_in_background": true})));
        let logs = t.event("ada", &Kind::ToolResult { id: "t2".into(), ok: true, output: "Async agent launched successfully.\nagentId: abc123 (internal)".into() });
        assert!(matches!(&logs[0].1, Kind::SubInfo { task, .. } if task == "abc123"));
        assert!(t.any(), "a background launch: still running");
        // its files: running, with a nested subagent of its own
        let line = |v: Value| v.to_string() + "\n";
        std::fs::write(dir.join("agent-abc123.meta.json"), json!({"toolUseId": "t2", "description": "long job", "requestShape": "background"}).to_string()).unwrap();
        std::fs::write(dir.join("agent-abc123.jsonl"), line(json!({"type": "user", "message": {"content": "do the job"}})) + &line(json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "id": "t3", "name": "Agent", "input": {"description": "helper"}}]}}))).unwrap();
        std::fs::write(dir.join("agent-def456.meta.json"), json!({"toolUseId": "t3", "description": "helper", "agentType": "Explore"}).to_string()).unwrap();
        std::fs::write(dir.join("agent-def456.jsonl"), line(json!({"type": "user", "message": {"content": "help"}})) + &line(json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "id": "x", "name": "Bash", "input": {"command": "cargo test"}}]}}))).unwrap();
        let sessions = [("ada".to_string(), "s1".to_string())];
        let logs = t.scan(&root, &sessions);
        assert!(logs.iter().any(|(_, k)| matches!(k, Kind::SubInfo { id, task, .. } if id == "t2" && task == "abc123")), "{logs:?}");
        assert!(logs.iter().any(|(_, k)| matches!(k, Kind::SubStart { id, parent: Some(p), prompt, .. } if id == "t3" && p == "t2" && prompt == "help")), "{logs:?}");
        assert_eq!(activity_of(&dir.join("agent-def456.jsonl")).as_deref(), Some("Bash cargo test"));
        assert!(t.scan(&root, &sessions).is_empty(), "0 news: 0 events");
        // both finish: the report of each
        std::fs::write(dir.join("agent-def456.meta.json"), json!({"toolUseId": "t3", "finished": true}).to_string()).unwrap();
        std::fs::write(dir.join("agent-abc123.meta.json"), json!({"toolUseId": "t2", "finished": true}).to_string()).unwrap();
        let mut f = std::fs::OpenOptions::new().append(true).open(dir.join("agent-abc123.jsonl")).unwrap();
        use std::io::Write;
        write!(f, "{}", line(json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "name": "SubagentHandback", "input": {"message": "all done"}}]}}))).unwrap();
        let logs = t.scan(&root, &sessions);
        assert!(logs.iter().any(|(_, k)| matches!(k, Kind::SubEnd { id, result, .. } if id == "t2" && result == "all done")), "{logs:?}");
        assert!(logs.iter().any(|(_, k)| matches!(k, Kind::SubEnd { id, .. } if id == "t3")), "{logs:?}");
        assert!(!t.any());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Claude Code's default: a background subagent without `run_in_background` in the input.
    /// Its launch result keeps it running (its master stays busy); its meta file ends it.
    #[test]
    fn a_launch_result_without_run_in_background_keeps_it_running() {
        let (mut t, _) = Tracker::from_log(&[]);
        t.event("ada", &tool("t1", json!({"description": "map keys", "prompt": "read"})));
        let out = "Async agent launched successfully. (This tool result is internal metadata.)\nagentId: abb626af5fcd5a2af (internal ID)\nThe agent is working in the background.";
        let logs = t.event("ada", &Kind::ToolResult { id: "t1".into(), ok: true, output: out.into() });
        assert!(matches!(&logs[0].1, Kind::SubInfo { task, .. } if task == "abb626af5fcd5a2af"), "{logs:?}");
        assert!(t.any() && t.masters().contains("ada"), "still running: the master is busy");
        assert!(t.running["t1"].background);
        // a report that names an agent id (0 launch): the end
        t.event("ada", &tool("t2", json!({"description": "x"})));
        let logs = t.event("ada", &Kind::ToolResult { id: "t2".into(), ok: true, output: "done: see agentId: x1 notes".into() });
        assert!(matches!(&logs[0].1, Kind::SubEnd { state, .. } if state == "done"));
    }

    #[test]
    fn a_dead_process_or_a_restarted_host_loses_its_subagents() {
        let (mut t, _) = Tracker::from_log(&[]);
        t.event("ada", &tool("t1", json!({"description": "a"})));
        t.event("bob", &tool("t2", json!({"description": "b"})));
        let logs = t.exit("ada", "the harness process ended");
        assert_eq!(logs.len(), 1);
        assert!(matches!(&logs[0].1, Kind::SubEnd { id, state, .. } if id == "t1" && state == "lost"));
        assert_eq!(t.masters(), HashSet::from(["bob".to_string()]));
        // a new host: the log's open subagents are lost
        let ev = |agent: &str, kind: Kind| Event { ts: 1, agent: Some(agent.into()), kind };
        let log = [
            ev("ada", Kind::SubStart { id: "a".into(), parent: None, description: "x".into(), agent_type: String::new(), background: true, prompt: String::new() }),
            ev("ada", Kind::SubStart { id: "b".into(), parent: None, description: "y".into(), agent_type: String::new(), background: false, prompt: String::new() }),
            ev("ada", Kind::SubEnd { id: "b".into(), state: "done".into(), result: String::new() }),
        ];
        let (t, logs) = Tracker::from_log(&log);
        assert_eq!(logs.len(), 1);
        assert!(matches!(&logs[0].1, Kind::SubEnd { id, state, .. } if id == "a" && state == "lost"));
        assert!(!t.any());
    }

    /// A background subagent ended; its master's `SendMessage` to its agent id resumes it. A
    /// `finished` meta file from before the resume does not end it; a new one does.
    #[test]
    fn a_message_to_an_ended_subagent_resumes_it_until_its_meta_file_ends_it_again() {
        let root = std::env::temp_dir().join(format!("codehead-subs-resume-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("proj/s1/subagents");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(root.join("proj/s1.jsonl"), "").unwrap();
        let sessions = [("ada".to_string(), "s1".to_string())];
        let (mut t, _) = Tracker::from_log(&[]);
        t.since = 0;
        t.event("ada", &tool("t1", json!({"description": "stream C"})));
        t.event("ada", &Kind::ToolResult { id: "t1".into(), ok: true, output: "Async agent launched successfully.\nagentId: c1 (internal)".into() });
        std::fs::write(dir.join("agent-c1.meta.json"), json!({"toolUseId": "t1", "finished": true}).to_string()).unwrap();
        std::fs::write(dir.join("agent-c1.jsonl"), "").unwrap();
        assert!(t.scan(&root, &sessions).iter().any(|(_, k)| matches!(k, Kind::SubEnd { id, .. } if id == "t1")));
        assert!(!t.any() && t.resumable());
        // a message to another id or of another agent: 0 resume
        let send = |to: &str| Kind::Tool { id: format!("m-{to}"), name: "SendMessage".into(), input: json!({"to": to, "message": "go on"}) };
        assert!(t.event("ada", &send("x9")).is_empty());
        assert!(t.event("bob", &send("c1")).is_empty());
        let logs = t.event("ada", &send("c1"));
        assert!(matches!(&logs[..], [(a, Kind::SubResume { id })] if a == "ada" && id == "t1"), "{logs:?}");
        assert_eq!(t.masters(), HashSet::from(["ada".to_string()]));
        // the old `finished` meta file (from before the resume): still running
        assert!(t.scan(&root, &sessions).is_empty());
        assert!(t.any());
        // a 2nd message while it runs: 0 2nd resume
        assert!(t.event("ada", &send("c1")).is_empty());
        // its new end
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(dir.join("agent-c1.meta.json"), json!({"toolUseId": "t1", "finished": true}).to_string()).unwrap();
        let logs = t.scan(&root, &sessions);
        assert!(logs.iter().any(|(_, k)| matches!(k, Kind::SubEnd { id, state, .. } if id == "t1" && state == "done")), "{logs:?}");
        assert!(!t.any());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A resume the events missed: the meta file of an ended subagent changed after its end, 0
    /// `finished`, its transcript grew: the scan resumes it.
    #[test]
    fn a_scan_resumes_an_ended_subagent_whose_files_changed_after_its_end() {
        let root = std::env::temp_dir().join(format!("codehead-subs-rescan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("proj/s1/subagents");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(root.join("proj/s1.jsonl"), "").unwrap();
        std::fs::write(dir.join("agent-d1.meta.json"), json!({"toolUseId": "t1"}).to_string()).unwrap();
        std::fs::write(dir.join("agent-d1.jsonl"), "").unwrap();
        let sessions = [("ada".to_string(), "s1".to_string())];
        let ev = |ts: u64, kind: Kind| Event { ts, agent: Some("ada".into()), kind };
        let now = crate::engine::now_ms();
        let transcript = dir.join("agent-d1.jsonl").display().to_string();
        // the log: it ended a minute ago (its files: changed since, 0 `finished`)
        let log = [
            ev(now - 120_000, Kind::SubStart { id: "t1".into(), parent: None, description: "stream D".into(), agent_type: String::new(), background: true, prompt: String::new() }),
            ev(now - 120_000, Kind::SubInfo { id: "t1".into(), transcript, task: "d1".into() }),
            ev(now - 60_000, Kind::SubEnd { id: "t1".into(), state: "done".into(), result: "r".into() }),
        ];
        let (mut t, lost) = Tracker::from_log(&log);
        assert!(lost.is_empty() && t.resumable());
        let logs = t.scan(&root, &sessions);
        assert!(matches!(&logs[..], [(_, Kind::SubResume { id })] if id == "t1"), "{logs:?}");
        assert!(t.any());
        // a log with the resume, the host restarts: lost, resumable again
        let mut log = log.to_vec();
        log.push(ev(now - 30_000, Kind::SubResume { id: "t1".into() }));
        let (t, lost) = Tracker::from_log(&log);
        assert!(matches!(&lost[..], [(_, Kind::SubEnd { id, state, .. })] if id == "t1" && state == "lost"), "{lost:?}");
        assert!(!t.any() && t.resumable());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn task_tools_of_the_harnesses() {
        for n in ["Agent", "Task", "task", "spawn_agent"] {
            assert!(is_sub_tool(n), "{n}");
        }
        for n in ["Bash", "TaskStop", "TodoWrite", "agents"] {
            assert!(!is_sub_tool(n), "{n}");
        }
    }
}
