use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
pub(crate) mod commits;
pub(crate) mod patch;
pub(crate) mod rank;
pub(crate) mod stash;
pub(crate) mod threads;
pub(crate) const MAX_UNTRACKED: usize = 300;
/// An untracked file larger than this: shown as binary.
pub(crate) const MAX_FILE: u64 = 512 * 1024;
/// Git's empty tree: the base of a branch with 0 commits.
pub(crate) const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Ctx,
    Add,
    Del,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DLine {
    pub kind: Kind,
    pub old: Option<u32>,
    pub new: Option<u32>,
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Hunk {
    /// The text after `@@ … @@` (the enclosing function, if git found one).
    pub ctx: String,
    pub old: u32,
    pub new: u32,
    pub lines: Vec<DLine>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Status {
    #[default]
    Modified,
    New,
    Deleted,
    Renamed,
}

/// What a file is, in review order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Cat {
    Schema,
    Logic,
    Config,
    Ui,
    Docs,
    Tests,
    Generated,
}

impl Cat {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Cat::Schema => "schema · API",
            Cat::Logic => "logic",
            Cat::Config => "config",
            Cat::Ui => "UI",
            Cat::Docs => "docs",
            Cat::Tests => "tests",
            Cat::Generated => "generated",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FileDiff {
    pub path: String,
    pub old_path: Option<String>,
    pub status: Status,
    pub binary: bool,
    pub hunks: Vec<Hunk>,
    pub add: usize,
    pub del: usize,
    pub cat: Cat,
    /// Hash of the file's diff: a review mark holds while it stays.
    pub hash: u64,
}

/// The changes of DIR's worktree against its base.
#[derive(Clone, Debug, Default)]
pub(crate) struct Changes {
    pub base: String,
    /// The base's commit (HEAD on the default branch).
    pub rev: String,
    /// Untracked paths (shown as new files).
    pub untracked: HashSet<String>,
    pub files: Vec<FileDiff>,
    /// Staged paths: true all of the file's uncommitted change, false a part.
    pub staged: HashMap<String, bool>,
    /// A rebase / merge stopped: its conflicted paths.
    pub stopped: Option<Vec<String>>,
    /// Commits ahead of / behind the base.
    pub ahead: usize,
    pub behind: usize,
    /// The branch's commits (the panel's COMMITS section).
    pub commits: commits::Commits,
    /// Ids of open comments on files with 0 uncommitted changes (they resolve).
    pub committed: Vec<String>,
    /// The checkout's git dir (its push lock).
    pub git_dir: Option<PathBuf>,
}

/// The branch's pull request (gh): number, url, state, checks passed / failed / pending.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Pr {
    pub number: u64,
    pub url: String,
    pub state: String,
    pub pass: usize,
    pub fail: usize,
    pub pending: usize,
}

/// A git step of the changes view (run on a thread; hooks, signing, ssh as the git CLI does).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Op {
    Stage(String),
    Unstage(String),
    Commit(String),
    Push,
    /// Fetch, then rebase on BASE (`origin/main`).
    Update(String),
    Continue,
    Abort,
    /// Stage / unstage the hunk of PATH that holds new-side LINE.
    StageHunk(String, u32),
    UnstageHunk(String, u32),
    /// Push, then `gh pr create --fill`.
    OpenPr,
    /// `gh pr merge --squash --auto` (merges when the checks pass).
    Merge,
    /// Stage / unstage these files.
    StageFiles(Vec<String>),
    UnstageFiles(Vec<String>),
    /// Stage the picked lines of each file (a part of its change).
    StageLines(Vec<(String, patch::Pick)>),
    /// Take the picked lines of each file out of the worktree into the stash of thread
    /// THREAD: (path, lines label, pick); the diff against REV.
    Stash {
        thread: String,
        rev: String,
        items: Vec<(String, String, patch::Pick)>,
    },
    /// Take these commits (oids) out of THREAD's history; HEAD: the HEAD they were picked on.
    Uncommit {
        thread: String,
        head: String,
        picks: Vec<String>,
    },
    /// Push the commits up to this oid.
    PushTo(String),
}

/// The toast of a push step while a push of the thread runs.
pub(crate) const PUSH_RUNS: &str = "a push runs in this thread: wait for it to end";

/// A review comment on LINE (new side) of PATH (LINES: the selected lines it is on), for the
/// batch to an agent.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Comment {
    pub path: String,
    pub line: u32,
    pub text: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub lines: String,
    /// The diff lines it is on, as the modal showed them (`+ let b = 3;`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub quote: String,
    /// Its severity (the panel groups the comments by it).
    #[serde(default)]
    pub level: rank::Level,
    /// The agent that wrote it ("": the owner).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub by: String,
    /// `c7`: agents name it in `comment_status` (an older file: given on read).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    /// Open / resolved, as on GitHub: a resolved comment leaves the file view.
    #[serde(default, skip_serializing_if = "crate::domain::serialization::is_default")]
    pub state: CState,
    /// Who resolved it ("": the owner), why (the agent's 1-line note).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub resolved_by: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    /// In a batch to the agents already (ctrl+enter sends only the new open ones).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sent: bool,
    /// The checkout PATH is in (absolute; "": an older comment, see `place`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub root: String,
    /// When it was written (unix ms; 0: an older comment).
    #[serde(default, skip_serializing_if = "crate::domain::serialization::is_default")]
    pub at: u64,
    /// The replies, oldest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub replies: Vec<Reply>,
}

/// A reply on a comment: its author (BY "": the owner), time (unix ms), text.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Reply {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub by: String,
    pub at: u64,
    pub text: String,
    /// An agent's reply the owner has not seen yet (a notification while true).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unseen: bool,
}

/// The note of a comment resolved by a commit.
const COMMITTED: &str = "the file has 0 uncommitted changes (committed or reverted)";

/// The state of a comment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum CState {
    #[default]
    Open,
    Resolved,
}

mod diff;
pub(crate) use diff::*;

mod repository;
pub(crate) use repository::*;

mod operations;
pub(crate) use operations::*;

mod prompts;
pub(crate) use prompts::*;

mod index;
pub(crate) use index::*;

mod comments;
pub(crate) use comments::*;

mod comment_commands;
pub(crate) use comment_commands::*;

mod categories;
pub(crate) use categories::categorize;
