use super::{Item, MAX_FILES, Path, PathBuf, age, secs};

/// Skills of HARNESSES as items, by harness + name (the 1st on top). `/name` goes into the text
/// (Codex: `$name`).
pub(crate) fn skills(harnesses: &[String], cwd: &Path) -> Vec<Item> {
    let all = crate::infra::skills::load(harnesses, cwd);
    let many = all.iter().any(|text| text.harness != all[0].harness);

    all.into_iter()
        .enumerate()
        .map(|(i, text_value)| {
            let sigil = if text_value.harness == "codex" { '$' } else { '/' };

            let label = if many {
                format!("{} · {}", text_value.name, text_value.harness)
            } else {
                text_value.name.clone()
            };

            Item {
                label,
                detail: text_value.description.clone(),
                insert: format!("{sigil}{}", text_value.name),
                time: i as u64,
                info: text_value.description,
            }
        })
        .collect()
}

/// Files of the git repo / worktree that holds CWD (tracked + untracked, ignored ones left out),
/// paths relative to its top dir. 0 git: the files below CWD. Oldest first.
pub(crate) fn files(cwd: &Path) -> Vec<Item> {
    let (ok, top, _) = crate::domain::threads::git(cwd, &["rev-parse", "--show-toplevel"]);
    let mut paths: Vec<(u64, String)> = Vec::new();

    if ok && !top.trim().is_empty() {
        let root = PathBuf::from(top.trim());
        let (_, out, _) =
            crate::domain::threads::git(&root, &["ls-files", "-z", "--cached", "--others", "--exclude-standard"]);

        for rel in out.split('\0').filter(|path| !path.is_empty()) {
            // a deleted, not yet committed file: listed by git, gone on disk
            append_file_metadata(&mut paths, &root, rel);
        }
    } else {
        walk(cwd, cwd, 0, &mut paths);
    }

    paths.sort();
    paths.dedup_by(|agent, bytes| agent.1 == bytes.1);
    let skip = paths.len().saturating_sub(MAX_FILES);

    paths
        .into_iter()
        .skip(skip)
        .map(|(time, rel)| Item {
            detail: age(time),
            insert: rel.clone(),
            label: rel,
            time,
            info: String::new(),
        })
        .collect()
}

fn append_file_metadata(paths: &mut Vec<(u64, String)>, root: &Path, rel: &str) {
    if let Ok(message) = std::fs::metadata(root.join(rel)) {
        paths.push((message.modified().map_or(0, secs), rel.to_string()));
    }
}

/// 0 git: files below DIR (hidden entries and build dirs left out, max. 6 levels).
pub(super) fn walk(root: &Path, dir: &Path, depth: usize, out: &mut Vec<(u64, String)>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };

    for error in rd.flatten() {
        if out.len() >= MAX_FILES {
            return;
        }

        let name = error.file_name().to_string_lossy().into_owned();

        if name.starts_with('.')
            || matches!(
                name.as_str(),
                "node_modules" | "target" | "dist" | "build" | "__pycache__"
            )
        {
            continue;
        }

        let Ok(meta) = error.metadata() else { continue };
        let path = error.path();

        if meta.is_dir() {
            walk_child_directory(root, depth, out, &path);
        } else {
            let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().into_owned();

            out.push((meta.modified().map_or(0, secs), rel));
        }
    }
}

fn walk_child_directory(root: &Path, depth: usize, out: &mut Vec<(u64, String)>, path: &Path) {
    if depth < 6 {
        walk(root, path, depth + 1, out);
    }
}

/// Notes of the notes root: shown relative, inserted as the full path. Oldest first.
pub(crate) fn notes(root: &Path) -> Vec<Item> {
    let count = crate::domain::notes::Notes::load(root);

    let mut value: Vec<Item> = count
        .files
        .iter()
        .map(|rel| {
            let time = secs(count.mtime(rel));

            Item {
                label: rel.clone(),
                detail: age(time),
                insert: count.abs(rel).display().to_string(),
                time,
                info: String::new(),
            }
        })
        .collect();

    value.sort_by_key(|i| i.time);
    value
}

/// Every thread: its name (1st line of the title). Oldest first.
pub(super) fn threads() -> Vec<Item> {
    let mut value: Vec<Item> = crate::domain::threads::all_threads_lite()
        .into_iter()
        .filter(|thread| !thread.title.trim().is_empty())
        .map(|thread_value| {
            let name = thread_value.title.lines().next().unwrap_or_default().trim().to_string();

            let place = thread_value
                .branch
                .clone()
                .or_else(|| {
                    thread_value
                        .dir
                        .as_deref()
                        .and_then(|directory| Path::new(directory).file_name())
                        .map(|count| count.to_string_lossy().into_owned())
                })
                .unwrap_or_default();

            let detail = [place, age(thread_value.updated)]
                .into_iter()
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>()
                .join(" · ");

            Item {
                label: name.clone(),
                detail,
                insert: name,
                time: thread_value.updated,
                info: String::new(),
            }
        })
        .collect();

    value.sort_by_key(|i| i.time);
    value
}
