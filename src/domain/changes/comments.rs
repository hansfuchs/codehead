use super::{COMMITTED, CState, Command, Comment, HashMap, HashSet, Path, PathBuf, Reply, threads};

impl Reply {
    /// The author for the owner's eyes: `you`, `@REGI`.
    pub(crate) fn author(&self) -> String {
        if self.by.is_empty() {
            "you".into()
        } else {
            format!("@{}", self.by)
        }
    }
}
/// The checkout (top dir of a worktree, linked ones included) that PATH is in.
pub(crate) fn checkout_of(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .filter(|dir| !dir.as_os_str().is_empty())
        .find(|dir| dir.join(".git").exists())
        .map(Path::to_path_buf)
}
/// Comment C's checkout and its path in it. None: an older comment with a relative path (the
/// thread's work dir, any checkout of the view).
pub(crate) fn place(comment: &Comment) -> (Option<PathBuf>, String) {
    if !comment.root.is_empty() {
        return (Some(PathBuf::from(&comment.root)), comment.path.clone());
    }

    let path = Path::new(&comment.path);

    if path.is_absolute()
        && let Some(checkout) = path.parent().and_then(checkout_of)
        && let Ok(rel) = path.strip_prefix(&checkout)
    {
        return (Some(checkout), rel.to_string_lossy().into_owned());
    }

    (None, comment.path.clone())
}
/// A and B: the same dir (/var = /private/var on macOS).
pub(crate) fn same_dir(left: &Path, right: &Path) -> bool {
    left == right
        || std::fs::canonicalize(left).is_ok_and(|canonical| std::fs::canonicalize(right).ok() == Some(canonical))
}
/// The paths of ROOT with uncommitted changes (staged, unstaged, untracked; both sides of a
/// rename). None: 0 git read.
pub(crate) fn uncommitted(root: &Path) -> Option<HashSet<String>> {
    // 0 index refresh: a write of the index would come back as a watcher topic (a loop)
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "--no-optional-locks",
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
        ])
        .output()
        .ok()?;

    if !out.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&out.stdout);
    let mut set = HashSet::new();
    let mut parts = text.split('\0').filter(|part| !part.is_empty());

    while let Some(entry) = parts.next() {
        let Some(path) = entry.get(3..) else { continue };

        set.insert(path.to_string());

        // a rename / copy: its source path follows
        if matches!(entry.as_bytes().first(), Some(b'R' | b'C'))
            && let Some(from) = parts.next()
        {
            set.insert(from.to_string());
        }
    }

    Some(set)
}
/// The ids of the open comments of COMMENTS on a file with 0 uncommitted changes in its
/// checkout (WORK: the checkout of older relative ones). A checkout git cannot read: 0 ids. A
/// comment whose last reply (the owner's) names an agent waits on it: 0 id.
pub(crate) fn committed(comments: &[Comment], work: &Path) -> Vec<String> {
    let mut reads: HashMap<PathBuf, Option<HashSet<String>>> = HashMap::new();
    let mut ids = Vec::new();

    for comment in comments
        .iter()
        .filter(|comment| comment.open() && !threads::waits_on_agent(comment))
    {
        let (root, rel) = place(comment);
        let root = root.unwrap_or_else(|| work.to_path_buf());
        let dirty = reads.entry(root.clone()).or_insert_with(|| uncommitted(&root));

        if dirty.as_ref().is_some_and(|dirty| !dirty.contains(&rel)) {
            ids.push(comment.id.clone());
        }
    }

    ids
}
/// Comments IDS of COMMENTS (open ones): resolved by codehead. True: one changed.
pub(crate) fn resolve_committed(comments: &mut [Comment], ids: &[String]) -> bool {
    let mut changed = false;

    for comment in comments
        .iter_mut()
        .filter(|comment| comment.open() && ids.contains(&comment.id))
    {
        comment.state = CState::Resolved;
        comment.resolved_by = "codehead".into();
        comment.note = COMMITTED.into();
        changed = true;
    }

    changed
}
/// THREAD's open comments on files with 0 uncommitted changes: resolved (WORK: its work dir).
pub(crate) fn settle_comments(thread: &str, work: &Path) {
    update_comments(thread, |all| {
        let ids = committed(all, work);

        resolve_committed(all, &ids);
    });
}
impl Comment {
    /// `11` or `11-13, 15`.
    pub(crate) fn at(&self) -> String {
        if self.lines.is_empty() {
            self.line.to_string()
        } else {
            self.lines.clone()
        }
    }
    pub(crate) fn open(&self) -> bool {
        self.state == CState::Open
    }
    /// The author for the owner's eyes: `you`, `@REGI`.
    pub(crate) fn author(&self) -> String {
        if self.by.is_empty() {
            "you".into()
        } else {
            format!("@{}", self.by)
        }
    }
}
pub(crate) fn comments_path(id: &str) -> PathBuf {
    crate::engine::store::thread_dir(id).join("comments.json")
}
/// The comments of thread ID; comments of an older file get their id (written back).
pub(crate) fn read_comments(id: &str) -> Vec<Comment> {
    let mut comments: Vec<Comment> = std::fs::read(comments_path(id))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();

    if comments.iter().any(|comment| comment.id.is_empty()) {
        give_ids(&mut comments);
        write_comments(id, &comments);
    }

    comments
}
/// Comments with 0 id get the next free `c<n>`.
pub(crate) fn give_ids(comments: &mut [Comment]) {
    let mut count = comments
        .iter()
        .filter_map(|comment| comment.id.strip_prefix('c')?.parse::<u32>().ok())
        .max()
        .unwrap_or(0);

    for comment in comments.iter_mut().filter(|comment| comment.id.is_empty()) {
        count += 1;
        comment.id = format!("c{count}");
    }
}
/// The comments file's time (an agent wrote a comment: the view reads it again).
pub(crate) fn comments_stamp(id: &str) -> Option<std::time::SystemTime> {
    std::fs::metadata(comments_path(id))
        .and_then(|metadata| metadata.modified())
        .ok()
}
pub(crate) fn write_comments(id: &str, comments: &[Comment]) {
    if let Ok(bytes) = serde_json::to_vec_pretty(comments) {
        let _ = crate::engine::write_atomic(&comments_path(id), &bytes);
    }
}
/// EDIT on thread ID's comments, read and written under the comments lock: views, hosts and MCP
/// servers write the same file (0 lost write). New comments get their id. A lock error: 0 lock.
pub(crate) fn update_comments<T>(id: &str, edit: impl FnOnce(&mut Vec<Comment>) -> T) -> T {
    let _lock = crate::engine::lock(&crate::engine::store::thread_dir(id).join("comments.lock"), true).ok();
    let mut all = read_comments(id);
    let before = all.clone();
    let out = edit(&mut all);

    give_ids(&mut all);

    if all != before {
        write_comments(id, &all);
    }

    out
}
/// The view's comments MINE (read as BASE) onto the file's DISK: per comment (by id) and field,
/// a field the view changed wins, else the file's. Comments the view dropped go, new ones (0 id or
/// an id BASE lacks) come at the end. Replies: always the file's (the view writes replies only
/// through `update_comments`).
pub(crate) fn merge_comments(base: &[Comment], mine: &[Comment], disk: Vec<Comment>) -> Vec<Comment> {
    let find = |list: &[Comment], id: &str| list.iter().find(|comment| !id.is_empty() && comment.id == id).cloned();

    let mut out: Vec<Comment> = Vec::new();

    for disk_comment in disk {
        let Some(base_comment) = find(base, &disk_comment.id) else {
            // new on disk since the view's read
            out.push(disk_comment);
            continue;
        };

        let Some(my_comment) = find(mine, &disk_comment.id) else {
            continue;
        };

        if my_comment == base_comment {
            out.push(disk_comment);
            continue;
        }

        let (bv, mv, dv) = (
            serde_json::to_value(&base_comment),
            serde_json::to_value(&my_comment),
            serde_json::to_value(&disk_comment),
        );

        let (
            Ok(serde_json::Value::Object(bv)),
            Ok(serde_json::Value::Object(mv)),
            Ok(serde_json::Value::Object(mut dv)),
        ) = (bv, mv, dv)
        else {
            out.push(my_comment);
            continue;
        };

        let keys: HashSet<String> = bv.keys().chain(mv.keys()).cloned().collect();

        for key in keys
            .into_iter()
            .filter(|key| key != "replies" && bv.get(key) != mv.get(key))
        {
            match mv.get(&key) {
                Some(value) => dv.insert(key, value.clone()),
                None => dv.remove(&key),
            };
        }

        out.push(serde_json::from_value(serde_json::Value::Object(dv)).unwrap_or(my_comment));
    }

    out.extend(
        mine.iter()
            .filter(|comment| comment.id.is_empty() || find(base, &comment.id).is_none())
            .cloned(),
    );
    out
}
