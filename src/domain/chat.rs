use serde_json::Value;
/// A subagent of an agent on the thread.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Sub {
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
    /// A rerun: its master resumed it after an earlier end (that run: an earlier `Sub`, same id).
    pub resumed: bool,
}

impl Sub {
    pub(crate) fn running(&self) -> bool {
        self.state.is_none()
    }
}

/// An open question or permission request.
#[derive(Clone, Debug)]
pub(crate) struct Ask {
    pub id: String,
    pub agent: String,
    pub tool: String,
    pub input: Value,
    /// When it came.
    pub ts: u64,
}

impl Ask {
    /// `AskUserQuestion`: its questions (text, header, options (label, description), multi).
    pub(crate) fn questions(&self) -> Vec<Question> {
        if self.tool != "AskUserQuestion" {
            return Vec::new();
        }

        self.input["questions"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|query| {
                let text = |key: &str| query[key].as_str().unwrap_or_default().to_string();

                let opts = question_options(query);

                (
                    text("question"),
                    text("header"),
                    opts,
                    query["multiSelect"].as_bool().unwrap_or(false),
                )
            })
            .collect()
    }

    /// An ACP agent's own choices: (option id, label, kind).
    pub(crate) fn options(&self) -> Vec<(String, String, String)> {
        self.input["_options"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|output| {
                let text = |key: &str| output[key].as_str().unwrap_or_default().to_string();

                (text("id"), text("name"), text("kind"))
            })
            .collect()
    }

    /// What a permission request asks, in 1 line.
    pub(crate) fn summary(&self) -> String {
        if self.tool == "ExitPlanMode" {
            return "leave plan mode and start on the plan".into();
        }

        // a flow's wait: its question
        if self.tool == crate::engine::flows::run::WAIT_TOOL {
            return self.input["question"].as_str().unwrap_or_default().to_string();
        }

        format!("{} {}", self.tool, call_summary(&self.tool, &self.input))
    }
}

/// How the stream shows the agents' thoughts (setting `thoughts`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Thoughts {
    /// 0 rows.
    #[default]
    Hidden,
    /// A folded row each (a click / ctrl+t opens it); in a run: a count, open with the run.
    Folded,
    /// Their full text, always.
    Open,
}

impl Thoughts {
    pub(crate) fn parse(text: &str) -> Option<Thoughts> {
        match text.trim() {
            "hidden" => Some(Thoughts::Hidden),
            "folded" => Some(Thoughts::Folded),
            "open" => Some(Thoughts::Open),
            _ => None,
        }
    }
}

/// How the stream draws messages (setting `messages`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Look {
    /// hell's: an agent's turn in a box (name, time in its top border), the owner's
    /// messages in a bubble on the right.
    #[default]
    Bubbles,
    /// A name badge, then the text behind a bar in the agent's color.
    Lines,
}

impl Look {
    pub(crate) fn parse(text: &str) -> Option<Look> {
        match text.trim() {
            "bubbles" => Some(Look::Bubbles),
            "lines" => Some(Look::Lines),
            _ => None,
        }
    }
}

pub(crate) fn clock(ts: u64) -> String {
    let secs = (ts / 1000) as libc::time_t;

    // SAFETY: localtime_r writes only into TM.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };

    unsafe { libc::localtime_r(&raw const secs, &raw mut tm) };
    format!("{:02}:{:02}", tm.tm_hour, tm.tm_min)
}

/// 1 line of what a call does: the command, the file, the pattern.
pub(crate) fn call_summary(name: &str, input: &Value) -> String {
    let text = |key: &str| input[key].as_str().map(String::from);

    let value_2 = match name {
        "Bash" => text("description").or_else(|| text("command")),
        "Read" | "Write" | "Edit" | "MultiEdit" | "NotebookEdit" => text("file_path")
            .or_else(|| text("notebook_path"))
            .map(|path| short_path(&path)),
        "Grep" | "Glob" => text("pattern"),
        "WebFetch" => text("url"),
        "WebSearch" => text("query"),
        "Task" | "Agent" => text("description"),
        "TodoWrite" => Some("todos".into()),
        _ if name.ends_with("codehead__set_projects") => Some(match input["projects"].as_array() {
            Some(agent) if !agent.is_empty() => agent.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", "),
            _ => "0 projects".into(),
        }),
        _ if name.ends_with("codehead__set_title") => text("title").map(|thread| match text("branch") {
            Some(bytes) => format!("{thread} · {bytes}"),
            None => thread,
        }),
        _ if name.starts_with("mcp__codehead__send") => Some(format!(
            "→ {}",
            match &input["to"] {
                Value::Array(agent_value) => agent_value
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", "),
                value => value.as_str().unwrap_or_default().to_string(),
            }
        )),
        _ => input.as_object().and_then(|output| {
            output
                .values()
                .find_map(|value_value| value_value.as_str().map(String::from))
        }),
    };

    value_2
        .unwrap_or_default()
        .lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(120)
        .collect()
}

fn short_path(path: &str) -> String {
    let cwd = std::env::current_dir()
        .map(|character| character.display().to_string() + "/")
        .unwrap_or_default();

    path.strip_prefix(&cwd).unwrap_or(path).to_string()
}

/// Draw T at WIDTH. OPEN: runs shown with their calls (ALL: every run). `ASK_AT`: the choice under
/// the cursor of the 1st open ask; PICKS: its picked choices (multi).
/// `needs harness: claude (Artifact tool)` in an agent's text: (harness, what for). The agents'
/// handoff rule asks for this line when 0 agent on the thread can do a step.
pub(crate) fn needs_harness(text: &str) -> Option<(String, String)> {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();

    let character = RE
        .get_or_init(|| {
            regex::Regex::new(r"(?i)\bneeds harness:\s*`?([a-z][a-z0-9_-]*)`?(?:\s*\(([^)]*)\))?").expect("valid regex")
        })
        .captures(text)?;

    Some((
        character[1].to_ascii_lowercase(),
        character
            .get(2)
            .map_or("", |message| message.as_str())
            .trim()
            .to_string(),
    ))
}

/// MS as `1h 05m 12s`, `12m 30s`, `45s`.
pub(crate) fn countdown(ms: u64) -> String {
    let text = ms.div_ceil(1000);

    match text {
        0..60 => format!("{text}s"),
        60..3600 => format!("{}m {:02}s", text / 60, text % 60),
        _ => format!("{}h {:02}m {:02}s", text / 3600, text / 60 % 60, text % 60),
    }
}

/// A usage or rate limit in an error text.
pub(crate) fn usage_limit(msg: &str) -> bool {
    let msg = msg.to_ascii_lowercase();

    [
        "hit your",
        "usage limit",
        "rate limit",
        "rate_limit",
        "quota exceeded",
        "too many requests",
    ]
    .iter()
    .any(|text| msg.contains(text))
}

pub(crate) const ATTACHMENTS: &str = "Attachments (read these files):";

/// A file attached to a prompt: its marker in the body (`[Image #1]`), its path.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Attachment {
    pub marker: Option<String>,
    pub path: String,
}

impl Attachment {
    /// 1 line under ATTACHMENTS: `[Image #1]: /p` (since the marker map), `/p` (older thread
    /// messages: a clipboard image's marker from its name `image-N.png`).
    fn parse(line: &str) -> Attachment {
        if line.starts_with('[')
            && let Some((message, path_value)) = line.split_once("]: ")
        {
            return Attachment {
                marker: Some(format!("{message}]")),
                path: path_value.trim().to_string(),
            };
        }

        let name = line.rsplit('/').next().unwrap_or(line);
        let count_2 = name
            .strip_prefix("image-")
            .and_then(|count| count.strip_suffix(".png"))
            .filter(|count_value| count_value.parse::<usize>().is_ok());

        Attachment {
            marker: count_2.map(|count_3| format!("[Image #{count_3}]")),
            path: line.to_string(),
        }
    }

    /// The file name.
    pub(crate) fn name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }
}

/// Prompt TEXT → (its body, the files attached under ATTACHMENTS).
pub(crate) fn attachments(text: &str) -> (&str, Vec<Attachment>) {
    match text.rfind(ATTACHMENTS) {
        Some(i) => (
            &text[..i],
            text[i + ATTACHMENTS.len()..]
                .lines()
                .map(|line| line.trim().trim_start_matches("- "))
                .filter(|line_value| !line_value.is_empty())
                .map(Attachment::parse)
                .collect(),
        ),
        None => (text, Vec::new()),
    }
}

type QuestionOption = (String, String);
type Question = (String, String, Vec<QuestionOption>, bool);

fn question_options(query: &Value) -> Vec<QuestionOption> {
    query["options"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|output| {
            (
                output["label"].as_str().unwrap_or_default().to_string(),
                output["description"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect()
}
