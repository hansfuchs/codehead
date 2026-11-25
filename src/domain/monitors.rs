use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::process::{Command, Stdio};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Watch {
    pub id: String,
    pub agent: String,
    pub harness: String,
    pub description: String,
    pub command: String,
    pub expires: Option<u64>,
    session: String,
}

impl Watch {
    /// Session + task id: 1 watch.
    pub(crate) fn key(&self) -> String {
        format!("{}:{}", self.session, self.id)
    }
}

/// End the processes running COMMAND (their command line holds its 1st line): each one with its
/// children, SIGTERM. Never codehead itself. The number ended.
pub(crate) fn kill(command: &str) -> usize {
    let needle = command
        .lines()
        .map(str::trim)
        .find(|line| line.len() >= 6)
        .unwrap_or_default();

    if needle.is_empty() {
        return 0;
    }

    let Ok(out) = Command::new("ps")
        .args(["-axwwo", "pid=,command="])
        .stdin(Stdio::null())
        .output()
    else {
        return 0;
    };

    let me = std::process::id();

    let pids: Vec<i32> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line_value| {
            let (pid, cmd) = line_value.trim_start().split_once(' ')?;
            let pid: u32 = pid.parse().ok()?;

            // the shell that runs the command, not a `ps` / `grep` that only names it
            (pid != me && cmd.contains(needle) && !cmd.starts_with("ps ")).then_some(pid as i32)
        })
        .collect();

    for &pid in &pids {
        // children first listed, the shell ended first (else it goes on to its next command)
        let children: Vec<i32> = Command::new("pgrep")
            .args(["-P", &pid.to_string()])
            .stdin(Stdio::null())
            .output()
            .map(|output_value| {
                String::from_utf8_lossy(&output_value.stdout)
                    .lines()
                    .filter_map(|line_2| line_2.trim().parse().ok())
                    .collect()
            })
            .unwrap_or_default();

        unsafe { libc::kill(pid, libc::SIGTERM) };

        for character in children {
            unsafe { libc::kill(character, libc::SIGTERM) };
        }
    }

    pids.len()
}

pub(crate) fn task_id(output: &Value) -> Option<String> {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();

    if let Some(id) = output
        .get("task_id")
        .or_else(|| output.get("taskId"))
        .and_then(Value::as_str)
    {
        return Some(id.to_string());
    }

    let re = RE.get_or_init(|| regex::Regex::new(r"(?:task |<task_id>|background with ID: )([A-Za-z0-9_-]+)").unwrap());

    re.captures(output.as_str()?).map(|character| character[1].to_string())
}

pub(crate) fn parse(rows: &[Value], now: u64) -> Vec<Watch> {
    let mut watches = HashMap::new();

    for row in rows {
        parse_watch(now, &mut watches, row);
    }

    let mut watches: Vec<_> = watches.into_values().collect();

    watches.sort_by(|agent_value, bytes| (&agent_value.agent, &agent_value.id).cmp(&(&bytes.agent, &bytes.id)));
    watches
}

fn parse_watch(now: u64, watches: &mut HashMap<String, Watch>, row: &Value) {
    let character = &row["content"];
    let input = &character["raw_input"];
    let name = character["title"].as_str().unwrap_or_default();
    let session = row["session"].as_str().unwrap_or_default();

    if name == "TaskStop" {
        remove_stopped_task(watches, input, session);

        return;
    }

    let codex = input["type"] == "commandExecution";
    let monitor = name.eq_ignore_ascii_case("monitor") || name.to_ascii_lowercase().ends_with("__monitor");
    let background = input["command"].is_string() && input["run_in_background"] == true;

    if !monitor && !background && !codex {
        return;
    }

    let Some(id) = (if codex {
        input["processId"].as_str().map(str::to_string)
    } else {
        task_id(&character["raw_output"])
    }) else {
        return;
    };

    let key = format!("{session}:{id}");
    let status = character["raw_output"]["status"]
        .as_str()
        .or(row["status"].as_str())
        .or(input["status"].as_str())
        .unwrap_or_default();

    if codex && !["inProgress", "in_progress", "running"].contains(&status) {
        watches.remove(&key);
        return;
    }

    let timeout = input["timeout_ms"]
        .as_u64()
        .or(if monitor { Some(1_800_000) } else { None });
    let expires = timeout.map(|thread| row["at"].as_u64().unwrap_or(now).saturating_add(thread));

    if expires.is_some_and(|error| error <= now) {
        return;
    }

    let command = input["command"].as_str().unwrap_or_default().to_string();

    watches.insert(
        key,
        Watch {
            id,
            agent: row["agent"].as_str().unwrap_or("agent").into(),
            harness: row["harness"].as_str().unwrap_or("unknown").into(),
            description: input["description"]
                .as_str()
                .unwrap_or_else(|| command.lines().next().unwrap_or("background task"))
                .chars()
                .map(description_character)
                .collect(),
            command,
            expires,
            session: session.into(),
        },
    );
}

fn description_character(character_value: char) -> char {
    if character_value.is_control() {
        ' '
    } else {
        character_value
    }
}

fn remove_stopped_task(watches: &mut HashMap<String, Watch>, input: &Value, session: &str) {
    if let Some(id) = input["task_id"].as_str() {
        watches.remove(&format!("{session}:{id}"));
    }
}

#[derive(Default)]
struct Transcript {
    offset: u64,
    tail: String,
    ended: HashSet<String>,
}

pub(crate) fn completed(path: &str) -> HashSet<String> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<HashMap<String, Transcript>>> = std::sync::OnceLock::new();

    let Ok(mut cache) = CACHE.get_or_init(Default::default).lock() else {
        return HashSet::new();
    };

    let Ok(mut file) = std::fs::File::open(path) else {
        return HashSet::new();
    };

    let entry = cache.entry(path.into()).or_default();

    if file.metadata().is_ok_and(|message| message.len() < entry.offset) {
        *entry = Transcript::default();
    }

    let _ = file.seek(SeekFrom::Start(entry.offset));
    let mut bytes = Vec::new();

    if file.read_to_end(&mut bytes).is_err() {
        return entry.ended.clone();
    }

    entry.offset += bytes.len() as u64;
    entry.tail.push_str(&String::from_utf8_lossy(&bytes));

    if let Some(end) = entry.tail.rfind('\n') {
        read_completed_tasks(entry, end);

        entry.tail.drain(..=end);
    }

    entry.ended.clone()
}

fn read_completed_tasks(entry: &mut Transcript, end: usize) {
    for line in entry.tail[..=end].lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };

        // the notice: queued, attached, or (delivered at once) a user message
        let text = match value["type"].as_str() {
            Some("queue-operation") => value["content"].as_str(),
            Some("attachment") => value["attachment"]["prompt"].as_str(),
            Some("user") if value["origin"]["kind"] == "task-notification" => value["message"]["content"].as_str(),
            _ => None,
        };

        let Some(text) = text.filter(|thread| thread.contains("<task-notification>")) else {
            continue;
        };

        record_completed_task(&mut entry.ended, text);
    }
}

fn record_completed_task(ended: &mut HashSet<String>, text: &str) {
    if ["completed", "failed", "killed", "cancelled", "stopped"]
        .iter()
        .any(|status| text.contains(&format!("<status>{status}</status>")))
        && let Some(id) = text
            .split_once("<task-id>")
            .and_then(|(_, thread_value)| thread_value.split_once("</task-id>"))
            .map(|(id, _)| id)
    {
        ended.insert(id.into());
    }
}

/// Watches of THREAD and of its running sub-threads (their agents marked `(sub)`).
/// None: a host did not answer in time (busy): its live agents unknown.
pub(crate) fn load(thread: &str) -> Option<Vec<Watch>> {
    let Some(thread_value) = crate::engine::store::load(thread) else {
        return Some(Vec::new());
    };

    let mut out = watches_of(&thread_value, &live(&thread_value.id)?);

    for kid in &thread_value.kids {
        let Some(key) = crate::engine::store::load(kid) else {
            continue;
        };

        let mut width = watches_of(&key, &live(&key.id)?);

        for x in &mut width {
            x.agent = format!("{} (sub)", x.agent);
        }

        out.extend(width);
    }

    Some(out)
}

/// Live agents of thread ID: 0 host → empty; a host with 0 answer in time → None.
pub(crate) fn live(id: &str) -> Option<Vec<String>> {
    crate::engine::client::state_known(id).map(|text| text.live)
}

/// Watches of thread ID alone (0 sub-thread), its live agents from its host.
pub(crate) fn watches(id: &str) -> Vec<Watch> {
    let Some(thread) = crate::engine::store::load(id) else {
        return Vec::new();
    };

    let (live, _) = crate::engine::client::state(&thread.id);

    watches_of(&thread, &live)
}

/// Thread ID's host itself: 1+ watch of its LIVE agents runs (a ping of its own socket: 0 answer).
pub(crate) fn watching(id: &str, live: &[String]) -> bool {
    crate::engine::store::load(id).is_some_and(|thread| !watches_of(&thread, live).is_empty())
}

/// Watches of thread T: its LIVE agents' Monitor calls and background commands, from the event
/// log, minus the tasks their Claude transcripts report ended.
pub(crate) fn watches_of(thread: &crate::engine::store::Thread, live: &[String]) -> Vec<Watch> {
    watches_at(thread, live, &crate::infra::config::home().join(".claude/projects"))
}

/// `watches_of`, the Claude transcripts under ROOT.
pub(crate) fn watches_at(thread: &crate::engine::store::Thread, live: &[String], root: &std::path::Path) -> Vec<Watch> {
    use crate::engine::store::{self, Kind};
    let mut rows: Vec<Value> = Vec::new();
    let mut at: HashMap<String, usize> = HashMap::new();

    for error in store::events(&thread.id) {
        let Some(agent) = error.agent.clone().filter(|agent_value| live.contains(agent_value)) else {
            continue;
        };

        match error.kind {
            Kind::Tool { id, name, input } => {
                record_monitor_call(&mut rows, &mut at, error.ts, &agent, id, &name, &input);
            }
            Kind::ToolResult { id, output, .. } => {
                record_monitor_result(&mut rows, &at, &id, output);
            }
            _ => {}
        }
    }

    let mut watches = parse(&rows, crate::engine::now_ms());

    let transcript = |agent: &str| -> Option<String> {
        crate::infra::claude::session_file(root, thread.sessions.get(agent)?)
            .map(|path_value| path_value.display().to_string())
    };

    let ended: HashMap<String, HashSet<String>> = watches
        .iter()
        .map(|width| width.session.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .filter_map(|agent_2| Some((agent_2.clone(), completed(&transcript(&agent_2)?))))
        .collect();

    watches.retain(|width_value| {
        !ended
            .get(&width_value.session)
            .is_some_and(|ids| ids.contains(&width_value.id))
    });

    watches
}

fn record_monitor_result(rows: &mut [Value], at: &HashMap<String, usize>, id: &str, output: String) {
    if let Some(&i) = at.get(id) {
        rows[i]["content"]["raw_output"] = Value::String(output);
    }
}

fn record_monitor_call(
    rows: &mut Vec<Value>,
    at: &mut HashMap<String, usize>,
    timestamp: u64,
    agent: &str,
    id: String,
    name: &str,
    input: &Value,
) {
    let watch = name.eq_ignore_ascii_case("monitor")
        || name.ends_with("__monitor")
        || name == "TaskStop"
        || input["run_in_background"] == true;

    if watch {
        at.insert(id, rows.len());
        rows.push(serde_json::json!({"session": agent, "agent": agent, "harness": "claude", "at": timestamp, "content": {"title": name, "raw_input": input}}));
    }
}
