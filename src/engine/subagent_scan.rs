use super::{
    Kind, Logs, Path, PathBuf, REDIR, RESUME_GAP, RESUME_WAIT, Run, Tracker, Value, mtime_ms, prompt_of, result_of,
};

impl Tracker {
    /// Claude Code: the subagent files of SESSIONS ((master, session id)) under ROOT. Gives
    /// transcripts, nested subagents, the ends of background and nested ones.
    pub(crate) fn scan(&mut self, root: &Path, sessions: &[(String, String)]) -> Logs {
        let mut logs = Logs::new();

        for (agent, session) in sessions {
            scan_session(root, session, agent, self, &mut logs);
        }

        logs
    }

    /// The running subagent whose task id is TASK (Claude Code's `agent_id` of a request).
    pub(crate) fn by_task(&self, task: &str) -> Option<(&String, &Run)> {
        self.running
            .iter()
            .find(|(_, run)| !task.is_empty() && run.task == task)
    }
}

fn scan_session(root: &Path, session: &str, agent: &str, tracker: &mut Tracker, logs: &mut Logs) {
    let Some(directory) = dir_of(root, session, tracker) else {
        return;
    };

    let Ok(entries) = std::fs::read_dir(&directory) else {
        return;
    };

    let mut paths = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.to_string_lossy().ends_with(".meta.json"))
        .collect::<Vec<_>>();

    paths.sort();

    for path in paths {
        if let Some(entry) = Metadata::read(path, &directory) {
            scan_entry(entry, agent, tracker, logs);
        }
    }
}

struct Metadata {
    path: PathBuf,
    transcript: PathBuf,
    tool: String,
    task: String,
    data: Value,
    finished: bool,
}

impl Metadata {
    fn read(path: PathBuf, directory: &Path) -> Option<Self> {
        let data: Value = serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?;
        let tool = data["toolUseId"].as_str()?.to_string();
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let task = name
            .trim_start_matches("agent-")
            .trim_end_matches(".meta.json")
            .to_string();
        let transcript = directory.join(format!("agent-{task}.jsonl"));
        let finished = data["finished"] == true;

        Some(Self {
            path,
            transcript,
            tool,
            task,
            data,
            finished,
        })
    }

    fn fresh(&self, resumed: u64) -> bool {
        resumed == 0
            || mtime_ms(&self.path) > resumed
            || (crate::engine::now_ms().saturating_sub(resumed) > RESUME_WAIT && mtime_ms(&self.transcript) <= resumed)
    }

    fn changed_after(&self, end: u64) -> bool {
        mtime_ms(&self.path) > end + RESUME_GAP && mtime_ms(&self.transcript) > end + RESUME_GAP
    }
}

fn scan_entry(entry: Metadata, agent: &str, tracker: &mut Tracker, logs: &mut Logs) {
    if let Some(run) = tracker.running.get_mut(&entry.tool).filter(|run| run.agent == agent) {
        attach_transcript(run, &entry, agent, logs);

        if entry.finished && entry.fresh(run.resumed) && (run.background || run.nested) {
            tracker.end(&entry.tool);
            logs.push((
                agent.to_string(),
                Kind::SubEnd {
                    id: entry.tool,
                    state: "done".into(),
                    result: result_of(&entry.transcript).unwrap_or_else(|| "(0 report)".into()),
                },
            ));
        }

        return;
    }

    if !entry.finished
        && tracker
            .done
            .get(&entry.tool)
            .is_some_and(|(run, end)| run.agent == agent && entry.changed_after(*end))
    {
        logs.extend(tracker.resume(&entry.tool));
        return;
    }

    if entry.finished
        || tracker.ended.contains(&entry.tool)
        || mtime_ms(&entry.transcript).max(mtime_ms(&entry.path)) < tracker.since
    {
        return;
    }

    register_nested(entry, agent, tracker, logs);
}

fn attach_transcript(run: &mut Run, entry: &Metadata, agent: &str, logs: &mut Logs) {
    if run.transcript.is_some() {
        return;
    }

    run.transcript = Some(entry.transcript.clone());
    run.task.clone_from(&entry.task);
    logs.push((
        agent.to_string(),
        Kind::SubInfo {
            id: entry.tool.clone(),
            transcript: entry.transcript.display().to_string(),
            task: entry.task.clone(),
        },
    ));
}

fn parent_transcript_contains(run: &Run, tool: &str) -> bool {
    run.transcript
        .as_ref()
        .is_some_and(|path| std::fs::read_to_string(path).is_ok_and(|text| text.contains(tool)))
}

fn register_nested(entry: Metadata, agent: &str, tracker: &mut Tracker, logs: &mut Logs) {
    let parent = tracker
        .running
        .iter()
        .filter(|(_, run)| run.agent == agent)
        .find(|(_, run)| parent_transcript_contains(run, &entry.tool))
        .map(|(id, _)| id.clone());
    let description = entry.data["description"]
        .as_str()
        .map_or_else(|| "subagent".into(), String::from);
    let background = entry.data["requestShape"] == "background";

    tracker.running.insert(
        entry.tool.clone(),
        Run {
            agent: agent.to_string(),
            description: description.clone(),
            background,
            nested: true,
            transcript: Some(entry.transcript.clone()),
            task: entry.task.clone(),
            resumed: 0,
        },
    );
    let agent_type = entry.data["agentType"].as_str().unwrap_or_default().to_string();

    logs.push((
        agent.to_string(),
        Kind::SubStart {
            id: entry.tool.clone(),
            parent,
            description,
            agent_type,
            background,
            prompt: prompt_of(&entry.transcript),
        },
    ));
    logs.push((
        agent.to_string(),
        Kind::SubInfo {
            id: entry.tool,
            transcript: entry.transcript.display().to_string(),
            task: entry.task,
        },
    ));
}

/// The `subagents` dir of Claude Code session SID under ROOT (`~/.claude/projects`).
/// A session resumed in another dir (the thread moved repo): the newest transcript copy, found
/// again after REDIR.
fn dir_of(root: &Path, sid: &str, model_state: &mut Tracker) -> Option<PathBuf> {
    if let Some((directory, _)) = model_state.dirs.get(sid).filter(|(_, at)| at.elapsed() < REDIR) {
        return Some(directory.clone());
    }

    let file = crate::infra::claude::session_file(root, sid)?;
    let directory_value = file.parent()?.join(sid).join("subagents");

    model_state
        .dirs
        .insert(sid.to_string(), (directory_value.clone(), std::time::Instant::now()));
    Some(directory_value)
}
