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

    for event in store::events(id) {
        match event.kind {
            Kind::Tool { name, input, .. } if WRITE_TOOLS.contains(&name.as_str()) || input["_kind"] == "edit" => {
                collect_written(dir, &mut wrote, &input);
            }
            Kind::Prompt { from, text, .. } if from == "owner" => {
                collect_attachments(&mut attached, &text);
            }
            _ => {}
        }
    }

    (wrote, attached)
}

fn collect_attachments(attached: &mut Vec<(PathBuf, Option<String>)>, text: &str) {
    for attachment in crate::domain::chat::attachments(text).1 {
        let path = PathBuf::from(&attachment.path);

        if !attached.iter().any(|(known, _)| *known == path) {
            attached.push((path, attachment.marker));
        }
    }
}

fn collect_written(dir: &Path, wrote: &mut Vec<PathBuf>, input: &serde_json::Value) {
    if let Some(named) = input["file_path"]
        .as_str()
        .or_else(|| input["path"].as_str())
        .or_else(|| input["notebook_path"].as_str())
    {
        let path = if Path::new(named).is_absolute() {
            PathBuf::from(named)
        } else {
            dir.join(named)
        };

        if !wrote.contains(&path) {
            wrote.push(path);
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

fn walk_entry(depth: usize, id: &str, out: &mut Vec<PathBuf>, entry: &std::fs::DirEntry) {
    let path = entry.path();
    let name = entry.file_name();

    if name.to_string_lossy().starts_with('.') {
        return;
    }

    if path.is_dir() && depth < 4 {
        walk(&path, depth + 1, id, out);
    } else if path.extension().is_some_and(|extension| extension == "md") {
        use std::io::{BufRead, BufReader};
        let first = std::fs::File::open(&path)
            .ok()
            .and_then(|file| BufReader::new(file).lines().next())
            .and_then(Result::ok)
            .unwrap_or_default();

        if first.starts_with("thread:") && first.contains(id) {
            out.push(path);
        }
    }
}
pub(super) fn mtime(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map_or(0, |elapsed| elapsed.as_secs())
}
/// The files of thread ID (DIR: its work dir): existing ones, newest first.
pub(crate) fn load(id: &str, dir: &Path) -> Vec<Entry> {
    let (wrote, attached) = log_files(id, dir);
    let mut entries: Vec<Entry> = wrote
        .into_iter()
        .filter(|path| path.is_file())
        .map(|path| Entry {
            at: mtime(&path),
            path,
            source: Source::Agent,
        })
        .collect();

    if let Some(home) = crate::infra::config::home_repo() {
        add_notes(id, &mut entries, &home);
    }

    // attachments: a file in another section too stays there (1 row per file)
    for (path, marker) in attached {
        if path.is_file() && !entries.iter().any(|entry| entry.path == path) {
            entries.push(Entry {
                at: mtime(&path),
                path,
                source: Source::Attached(marker),
            });
        }
    }

    entries.sort_by_key(|entry| std::cmp::Reverse(entry.at));

    entries
}

fn add_notes(id: &str, entries: &mut Vec<Entry>, home: &Path) {
    for path in notes_of(home, id) {
        if !entries.iter().any(|entry| entry.path == path) {
            entries.push(Entry {
                at: mtime(&path),
                path,
                source: Source::Note,
            });
        }
    }
}

fn walk(dir: &Path, depth: usize, id: &str, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };

    for entry in rd.flatten() {
        walk_entry(depth, id, out, &entry);
    }
}
