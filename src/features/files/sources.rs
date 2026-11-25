use super::{Kind, Path, PathBuf, SystemTime, store};

/// Where a file comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    /// An agent's Write / Edit call.
    Agent,
    /// A home repo note of this thread.
    Note,
    /// A file a message named (a click on its link).
    Named,
    /// A file the owner attached to a message (a paste, a drop), with its marker (`[Image #1]`).
    Attached(Option<String>),
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Entry {
    pub path: PathBuf,
    pub source: Source,
    /// Last change, unix s.
    pub at: u64,
}
/// Tool inputs that name the file they write.
pub(super) const WRITE_TOOLS: [&str; 5] = [
    "Write",
    "Edit",
    "MultiEdit",
    "NotebookEdit",
    "str_replace_based_edit_tool",
];
/// The files of thread ID's log, 1 read: the agents wrote or edited (absolute; relative ones
/// from DIR), the owner attached to messages (with markers); each once.
pub(crate) fn log_files(id: &str, dir: &Path) -> (Vec<PathBuf>, Vec<(PathBuf, Option<String>)>) {
    let mut wrote: Vec<PathBuf> = Vec::new();
    let mut attached: Vec<(PathBuf, Option<String>)> = Vec::new();

    for error in store::events(id) {
        match error.kind {
            Kind::Tool { name, input, .. } if WRITE_TOOLS.contains(&name.as_str()) || input["_kind"] == "edit" => {
                log_files_tool_name_input_if(dir, &mut wrote, &input);
            }
            Kind::Prompt { from, text, .. } if from == "owner" => {
                log_files_prompt_from_text_if(&mut attached, &text);
            }
            _ => {}
        }
    }

    (wrote, attached)
}

fn log_files_prompt_from_text_if(attached: &mut Vec<(PathBuf, Option<String>)>, text: &str) {
    for agent in crate::domain::chat::attachments(text).1 {
        let path_3 = PathBuf::from(&agent.path);

        if !attached.iter().any(|(query, _)| *query == path_3) {
            attached.push((path_3, agent.marker));
        }
    }
}

fn log_files_tool_name_input_if(dir: &Path, wrote: &mut Vec<PathBuf>, input: &serde_json::Value) {
    if let Some(path_value) = input["file_path"]
        .as_str()
        .or_else(|| input["path"].as_str())
        .or_else(|| input["notebook_path"].as_str())
    {
        let path_2 = if Path::new(path_value).is_absolute() {
            PathBuf::from(path_value)
        } else {
            dir.join(path_value)
        };

        if !wrote.contains(&path_2) {
            wrote.push(path_2);
        }
    }
}
/// Files the agents of thread ID wrote or edited (absolute; relative ones from DIR), each once.
#[cfg(test)]
pub(crate) fn agent_files(id: &str, dir: &Path) -> Vec<PathBuf> {
    log_files(id, dir).0
}
/// Markdown files under ROOT whose line 1 names thread ID (`thread: <id>`), at most 4 dirs deep.
pub(crate) fn notes_of(root: &Path, id: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();

    walk(root, 0, id, &mut out);
    out
}

fn walk_path_(depth: usize, id: &str, out: &mut Vec<PathBuf>, error: &std::fs::DirEntry) {
    let path_2 = error.path();
    let name = error.file_name();

    if name.to_string_lossy().starts_with('.') {
        return;
    }

    if path_2.is_dir() && depth < 4 {
        walk(&path_2, depth + 1, id, out);
    } else if path_2.extension().is_some_and(|x| x == "md") {
        use std::io::{BufRead, BufReader};
        let first = std::fs::File::open(&path_2)
            .ok()
            .and_then(|file| BufReader::new(file).lines().next())
            .and_then(Result::ok)
            .unwrap_or_default();

        if first.starts_with("thread:") && first.contains(id) {
            out.push(path_2);
        }
    }
}
pub(super) fn mtime(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|message| message.modified())
        .ok()
        .and_then(|thread| thread.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map_or(0, |directory| directory.as_secs())
}
/// The files of thread ID (DIR: its work dir): existing ones, newest first.
pub(crate) fn load(id: &str, dir: &Path) -> Vec<Entry> {
    let (wrote, attached) = log_files(id, dir);
    let mut value: Vec<Entry> = wrote
        .into_iter()
        .filter(|path_value| path_value.is_file())
        .map(|path_2| Entry {
            at: mtime(&path_2),
            path: path_2,
            source: Source::Agent,
        })
        .collect();

    if let Some(home) = crate::infra::config::home_repo() {
        load_body(id, &mut value, &home);
    }

    // attachments: a file in another section too stays there (1 row per file)
    for (path_4, marker) in attached {
        if path_4.is_file() && !value.iter().any(|error_value| error_value.path == path_4) {
            value.push(Entry {
                at: mtime(&path_4),
                path: path_4,
                source: Source::Attached(marker),
            });
        }
    }

    value.sort_by_key(|error_2| std::cmp::Reverse(error_2.at));

    value
}

fn load_body(id: &str, value: &mut Vec<Entry>, home: &Path) {
    for path_3 in notes_of(home, id) {
        if !value.iter().any(|error| error.path == path_3) {
            value.push(Entry {
                at: mtime(&path_3),
                path: path_3,
                source: Source::Note,
            });
        }
    }
}

fn walk(path_value: &Path, depth: usize, id: &str, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(path_value) else { return };

    for error in rd.flatten() {
        walk_path_(depth, id, out, &error);
    }
}
