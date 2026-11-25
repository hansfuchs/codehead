//! More checkouts per thread: an agent edits a file in a git repo outside the thread's work dir
//! (another repo, another worktree) → that checkout joins the thread (`Thread::checkouts`), with
//! its branch. The merge tab finds the PRs of every checkout's branch; Claude agents get each
//! checkout as `--add-dir`.
//!
//! Edits only (Edit / Write / `MultiEdit` / `NotebookEdit`, ACP `edit` tools, Bash commands that
//! write: `git commit`, `sed -i`, `> file`, `mv` …, in their `cd DIR` / `git -C DIR`): reads of
//! other repos (research: `cd DIR && grep`, `git -C DIR status`) add 0 checkouts.

use std::collections::HashSet;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

use serde_json::Value;

use super::store;

/// The absolute paths a tool call NAME with INPUT edits (or works in).
pub(crate) fn edited(name: &str, input: &Value) -> Vec<PathBuf> {
    let path = |key: &str| input[key].as_str().map(PathBuf::from);
    let mut out = Vec::new();
    let edit = matches!(name, "Edit" | "Write" | "MultiEdit" | "NotebookEdit") || input["_kind"] == "edit";

    if edit {
        out.extend(
            ["file_path", "notebook_path", "path"]
                .iter()
                .filter_map(|key_value| path(key_value)),
        );

        // ACP: the edit's locations
        for line in input["locations"].as_array().into_iter().flatten() {
            parse_edited_path(&mut out, line);
        }
    }

    if name == "Bash"
        && let Some(cmd) = input["command"].as_str()
    {
        out.extend(work_dirs(cmd));
    }

    out.retain(|path_2| path_2.is_absolute());

    out
}

fn parse_edited_path(out: &mut Vec<PathBuf>, line: &Value) {
    if let Some(path_value) = line["path"].as_str() {
        out.push(PathBuf::from(path_value));
    }
}

/// Git subcommands that change the checkout (its files, index or branch) or publish it.
const GIT_WRITES: &[&str] = &[
    "add",
    "am",
    "apply",
    "checkout",
    "cherry-pick",
    "commit",
    "merge",
    "mv",
    "pull",
    "push",
    "rebase",
    "reset",
    "restore",
    "revert",
    "rm",
    "stash",
    "switch",
];

/// Shell commands that write their path arguments.
const SHELL_WRITES: &[&str] = &[
    "chmod", "cp", "install", "ln", "mkdir", "mv", "patch", "rm", "rmdir", "tee", "touch", "truncate",
];

/// CMD without the bodies of its heredocs (`<<EOF` … `EOF`): their lines are text, not commands.
fn strip_heredocs(cmd: &str) -> String {
    let mut out = String::new();
    let mut end: Option<String> = None;

    for line in cmd.lines() {
        if end.is_some() {
            clear_heredoc_end(&mut end, line);

            continue;
        }

        out.push_str(line);
        out.push('\n');

        if let Some(i) = line.find("<<") {
            let rest = line[i + 2..].trim_start_matches(['-', '~']).trim_start();
            let word: String = rest
                .chars()
                .take_while(|character| !character.is_whitespace() && !matches!(character, ';' | '&' | '|' | ')'))
                .collect();
            let word = word.trim_matches(|character_value| character_value == '"' || character_value == '\'');

            skip_heredoc_token(&mut end, word);
        }
    }

    out
}

fn skip_heredoc_token(end: &mut Option<String>, word: &str) {
    if !word.is_empty() && !word.starts_with('<') {
        *end = Some(word.to_string());
    }
}

fn clear_heredoc_end(end: &mut Option<String>, line: &str) {
    if end.as_deref() == Some(line.trim()) {
        *end = None;
    }
}

/// Word W without its surrounding quotes.
fn unquote(width: &str) -> &str {
    for query in ['"', '\''] {
        if width.len() >= 2 && width.starts_with(query) && width.ends_with(query) {
            return &width[1..width.len() - 1];
        }
    }

    width
}

/// Path word W, absolute or under CWD (`~/` → home); 0 path: None.
fn path_of(width: &str, cwd: Option<&Path>) -> Option<PathBuf> {
    let width_value = unquote(width);

    if width_value.is_empty() || width_value.starts_with('-') || width_value.starts_with('$') {
        return None;
    }

    if let Some(result) = width_value.strip_prefix("~/") {
        return Some(crate::infra::config::home().join(result));
    }

    let path = PathBuf::from(width_value);

    if path.is_absolute() {
        return Some(path);
    }

    cwd.map(|character| character.join(path))
}

/// The dirs and files shell command CMD writes in: per segment (`&&`, `;`, `|` …) that writes,
/// its dir (`cd DIR` before it, `git -C DIR`) and its absolute targets (`> /x`, `mv a /x`).
/// Segments that only read (`cd DIR && grep`, `git -C DIR status`): 0 paths.
fn work_dirs(cmd: &str) -> Vec<PathBuf> {
    let cmd = strip_heredocs(cmd);
    let mut out = Vec::new();
    let cwd: Option<PathBuf> = None;
    let segments = cmd.split([';', '&', '|', '(', ')', '\n']);

    scan_shell_segments(&mut out, cwd, segments);

    out.retain(|path| !path.starts_with("/dev"));

    out
}

fn scan_shell_segments(out: &mut Vec<PathBuf>, mut cwd: Option<PathBuf>, segments: std::str::Split<'_, [char; 6]>) {
    for seg in segments {
        let words: Vec<&str> = seg.split_whitespace().collect();

        // leading `VAR=x` assignments, `sudo`, `command`: not the command
        let Some(at) = words.iter().position(|word| {
            !(word.contains('=') && !word.starts_with('-')) && !matches!(*word, "sudo" | "command" | "time" | "env")
        }) else {
            continue;
        };

        let (verb, args) = (words[at], &words[at + 1..]);

        if verb == "cd" {
            cwd = args
                .first()
                .and_then(|width_value| path_of(width_value, cwd.as_deref()));
            continue;
        }

        let mut dir = cwd.clone();
        let (mut writes, mut targets) = redirect_targets(&words);

        match verb {
            "git" => {
                collect_git_targets(cwd.as_deref(), args, &mut dir, &mut writes);
            }
            "sed" | "perl" => {
                collect_script_targets(verb, args, &mut targets, &mut writes);
            }
            value if SHELL_WRITES.contains(&value) => {
                collect_command_targets(args, &mut targets, &mut writes, value);
            }
            _ => {}
        }

        if writes {
            out.extend(dir);
            out.extend(targets);
        }
    }
}

fn skip_redirect_target(target: &str) -> ControlFlow<()> {
    if target.starts_with('&') || unquote(target).starts_with("/dev/") || target.is_empty() {
        return ControlFlow::Break(());
    }

    ControlFlow::Continue(())
}

fn collect_command_targets(args: &[&str], targets: &mut Vec<PathBuf>, writes: &mut bool, value: &str) {
    *writes = true;
    let paths: Vec<PathBuf> = args.iter().filter_map(|width_8| path_of(width_8, None)).collect();

    // a copy writes its last argument only, the sources stay
    match value {
        "cp" | "install" | "ln" => targets.extend(paths.last().cloned()),
        _ => targets.extend(paths),
    }
}

fn collect_git_targets(cwd: Option<&Path>, args: &[&str], dir: &mut Option<PathBuf>, writes: &mut bool) {
    let mut i = 0;

    while i < args.len() && args[i].starts_with('-') {
        if args[i] == "-C" {
            *dir = args.get(i + 1).and_then(|width_3| path_of(width_3, cwd));
        }

        i += if matches!(args[i], "-C" | "-c") { 2 } else { 1 };
    }

    *writes |= args.get(i).is_some_and(|text| GIT_WRITES.contains(text));
}

fn collect_script_targets(verb: &str, args: &[&str], targets: &mut Vec<PathBuf>, writes: &mut bool) {
    // `-i`, `-i.bak`, `--in-place`; perl: `-pi`, `-pie` (lower-case flag clusters)
    let perl_flags = |width_4: &str| {
        width_4.len() > 1
            && width_4[1..]
                .chars()
                .all(|character_value| character_value.is_ascii_lowercase())
            && width_4.contains('i')
    };

    let in_place = |width_5: &&str| {
        *width_5 == "--in-place"
            || (width_5.starts_with('-')
                && !width_5.starts_with("--")
                && (width_5.starts_with("-i") || (verb == "perl" && perl_flags(width_5))))
    };

    if args.iter().any(in_place) {
        *writes = true;
        targets.extend(
            args.iter()
                .filter(|width_6| unquote(width_6).starts_with('/') || width_6.starts_with("~/"))
                .filter_map(|width_7| path_of(width_7, None)),
        );
    }
}

/// The git toplevel of path P (a file or a dir) and its branch.
fn toplevel(path: &Path) -> Option<(PathBuf, Option<String>)> {
    let mut directory = path.to_path_buf();

    while !directory.is_dir() {
        directory = directory.parent()?.to_path_buf();
    }

    let git = |args: &[&str]| {
        Command::new("git")
            .arg("-C")
            .arg(&directory)
            .args(args)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()
            .filter(|output_value| output_value.status.success())
            .map(|output_2| String::from_utf8_lossy(&output_2.stdout).trim().to_string())
    };

    let top = PathBuf::from(git(&["rev-parse", "--show-toplevel"])?);
    let branch = git(&["rev-parse", "--abbrev-ref", "HEAD"]).filter(|bytes| bytes != "HEAD");

    Some((top, branch))
}

/// Path P sits in a linked worktree nested inside checkout C (`.claude/worktrees/x`; a submodule
/// or a nested repo: 0, they stay the checkout's). Filesystem only.
fn nested(path: &Path, path_value: &Path) -> bool {
    path.ancestors()
        .take_while(|agent| *agent != path_value && agent.starts_with(path_value))
        .any(crate::domain::threads::linked_worktree)
}

/// Tool call NAME / INPUT of thread T: its edits outside T's checkouts add theirs.
pub(crate) fn note(thread: &store::Thread, name: &str, input: &Value) {
    // a dir checked once per process: 0 more git calls
    static SEEN: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);
    let have: Vec<PathBuf> = thread
        .all_checkouts()
        .iter()
        .map(|path| PathBuf::from(&path.dir))
        .collect();
    let data = super::data_dir();

    // codehead's own files: 0 checkout; its worktrees (`worktrees/<repo>/<task>`): checkouts
    let own = |path_value: &Path| path_value.starts_with(&data) && !path_value.starts_with(data.join("worktrees"));

    for path_2 in edited(name, input) {
        // a worktree / repo inside a checkout (`.claude/worktrees/x`): a checkout of its own
        if have
            .iter()
            .any(|character| path_2.starts_with(character) && !nested(&path_2, character))
            || own(&path_2)
        {
            continue;
        }

        let dir = if path_2.is_dir() {
            path_2.clone()
        } else {
            path_2.parent().map(Path::to_path_buf).unwrap_or_default()
        };

        let group = SEEN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);

        if let ControlFlow::Break(()) = claim_checkout_path(dir, group) {
            continue;
        }

        let Some((top, branch)) = toplevel(&path_2) else {
            continue;
        };

        if have.contains(&top) {
            continue;
        }

        let character_value = store::Checkout {
            dir: top.display().to_string(),
            branch,
        };

        let _ = store::update(&thread.id, move |thread_value| {
            thread_value.add_checkout(character_value);
        });
    }
}

fn claim_checkout_path(
    dir: PathBuf,
    mut group: std::sync::MutexGuard<'_, Option<HashSet<PathBuf>>>,
) -> ControlFlow<()> {
    if !group.get_or_insert_with(HashSet::new).insert(dir) {
        return ControlFlow::Break(());
    }

    ControlFlow::Continue(())
}

/// CHECKOUTS of T that 0 edit in EVENTS falls in (added by a read of an older version).
fn unedited(thread: &store::Thread, events: &[store::Event]) -> Vec<String> {
    let paths: Vec<PathBuf> = events
        .iter()
        .filter_map(|error| match &error.kind {
            store::Kind::Tool { name, input, .. } => Some(edited(name, input)),
            _ => None,
        })
        .flatten()
        .collect();

    thread
        .checkouts
        .iter()
        .filter(|character| !paths.iter().any(|path| path.starts_with(&character.dir)))
        .map(|character_value| character_value.dir.clone())
        .collect()
}

/// Thread T once: drops the checkouts its events hold 0 edit in. Changed: the thread as saved.
/// 0 checkouts: 0 write (the checkouts of later edits hold their edit).
pub(crate) fn prune(thread: &store::Thread) -> Option<store::Thread> {
    if thread.checkouts_pruned || thread.checkouts.is_empty() {
        return None;
    }

    // a handoff thread (`parent`) took its checkouts over: their edits sit in the parents' events
    let mut events = store::events(&thread.id);
    let mut up = thread.parent.clone();
    let mut seen = HashSet::from([thread.id.clone()]);

    while let Some(id) = up.filter(|id| seen.insert(id.clone())) {
        events.extend(store::events(&id));
        up = store::load(&id).and_then(|path| path.parent);
    }

    let gone = unedited(thread, &events);

    // under the thread's lock, not `store::update`: the thread's `updated` time stays (0 jump to
    // the top of the thread list); a checkout added since the read stays, only GONE goes
    super::locked(&store::thread_dir(&thread.id).join("thread.lock"), || {
        let mut thread_value = store::load(&thread.id)?;

        thread_value
            .checkouts
            .retain(|character| !gone.contains(&character.dir));
        thread_value.checkouts_pruned = true;
        thread_value.save().ok()?;
        Some(thread_value)
    })
    .ok()
    .flatten()
}

#[cfg(test)]
#[path = "checkouts_tests.rs"]
mod tests;

// redirects: `> f`, `>> f`, `2> f`, `>f`; 0 for `>&2`, `/dev/…`
fn redirect_targets(words: &[&str]) -> (bool, Vec<PathBuf>) {
    let mut targets = Vec::new();
    let mut writes = false;

    for (i, width_2) in words.iter().enumerate() {
        let Some(result) = width_2.find('>') else { continue };

        if !width_2[..result]
            .chars()
            .all(|character| character.is_ascii_digit() || character == '&')
        {
            continue;
        }

        let rest = width_2[result..].trim_start_matches('>');

        let target = if rest.is_empty() {
            words.get(i + 1).copied().unwrap_or_default()
        } else {
            rest
        };

        if let ControlFlow::Break(()) = skip_redirect_target(target) {
            continue;
        }

        writes = true;
        targets.extend(path_of(target, None));
    }

    (writes, targets)
}
