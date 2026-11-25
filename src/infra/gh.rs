//! GitHub pull requests over the gh CLI: 1 GraphQL query per PR (everything the merge tab
//! shows), 1 per list (the owner's PRs), the edits as `gh pr …` / REST calls. Every call blocks:
//! the callers run them off the UI thread.
//!
//! The merge tab keeps the calls per minute down (merge.rs).

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

/// A pull request: repo (`owner/name`), number.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub(crate) struct PrRef {
    pub repo: String,
    pub number: u64,
}

impl PrRef {
    pub(crate) fn new(repo: &str, number: u64) -> PrRef {
        PrRef {
            repo: repo.to_string(),
            number,
        }
    }

    /// The tab label: `name#123`.
    pub(crate) fn label(&self) -> String {
        format!("{}#{}", self.repo.rsplit('/').next().unwrap_or(&self.repo), self.number)
    }

    fn owner_name(&self) -> (&str, &str) {
        self.repo.split_once('/').unwrap_or(("", &self.repo))
    }
}

/// Every PR URL in TEXT (`github.com/owner/name/pull/123`), in order, 0 dupes.
pub(crate) fn pr_urls(text: &str) -> Vec<PrRef> {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();

    let re = RE.get_or_init(|| {
        regex::Regex::new(r"github\.com/([A-Za-z0-9_.-]+)/([A-Za-z0-9_.-]+)/pull/([0-9]+)").expect("valid")
    });

    let mut out: Vec<PrRef> = Vec::new();

    for captures in re.captures_iter(text) {
        let pr = PrRef::new(
            &format!("{}/{}", &captures[1], &captures[2]),
            captures[3].parse().unwrap_or(0),
        );

        if pr.number > 0 && !out.contains(&pr) {
            out.push(pr);
        }
    }

    out
}

/// `owner/name` of a git remote URL (ssh or https, github.com only).
pub(crate) fn repo_of_remote(url: &str) -> Option<String> {
    let trimmed = url.trim().trim_end_matches('/').trim_end_matches(".git");
    let rest = trimmed
        .strip_prefix("git@github.com:")
        .or_else(|| trimmed.split_once("github.com/").map(|(_, path)| path))
        .or_else(|| trimmed.strip_prefix("ssh://git@github.com/"))?;
    let mut parts = rest.split('/');
    let (owner, name) = (parts.next()?, parts.next()?);

    (!owner.is_empty() && !name.is_empty()).then(|| format!("{owner}/{name}"))
}

/// `owner/name` of the checkout DIR (its `origin`, else its 1st GitHub remote).
pub(crate) fn repo_of_dir(dir: &Path) -> Option<String> {
    let git = |args: &[&str]| {
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
    };

    if let Some(repo) = git(&["remote", "get-url", "origin"]).and_then(|url| repo_of_remote(&url)) {
        return Some(repo);
    }

    git(&["remote", "-v"])?
        .lines()
        .find_map(|line| line.split_whitespace().nth(1).and_then(repo_of_remote))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum State {
    #[default]
    Open,
    Draft,
    Merged,
    Closed,
}

impl State {
    fn of(state: &str, draft: bool) -> State {
        match state {
            "MERGED" => State::Merged,
            "CLOSED" => State::Closed,
            _ if draft => State::Draft,
            _ => State::Open,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            State::Open => "open",
            State::Draft => "draft",
            State::Merged => "merged",
            State::Closed => "closed",
        }
    }
}

/// A row of the owner's PR list.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Item {
    pub pr: PrRef,
    pub title: String,
    pub state: State,
    /// Unix s.
    pub created: u64,
    /// The head branch.
    pub head: String,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Label {
    pub name: String,
    /// `rrggbb`.
    pub color: String,
}

/// Where a comment lives (the API to edit, delete, reply).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum Kind {
    /// On the conversation (REST: issues/comments).
    #[default]
    Issue,
    /// The body of a review (REST: pulls/reviews; 0 delete).
    Review,
    /// On a diff line, in a thread (REST: pulls/comments).
    Line,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Comment {
    pub kind: Kind,
    pub id: u64,
    pub author: String,
    pub body: String,
    /// Unix s.
    pub created: u64,
    pub url: String,
    pub mine: bool,
    /// A review: its verdict (`APPROVED`, `CHANGES_REQUESTED`, `COMMENTED`).
    pub verdict: String,
}

/// A comment of the conversation with its replies (a review thread: the 1st comment and the
/// rest), PATH / LINE for a thread on the diff.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Thread {
    pub root: Comment,
    pub replies: Vec<Comment>,
    pub path: Option<String>,
    pub line: Option<u32>,
    pub resolved: bool,
    pub outdated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub(crate) enum Verdict {
    Fail,
    Pending,
    Pass,
    Skip,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Check {
    pub name: String,
    /// The workflow (Actions) or the app.
    pub group: String,
    pub verdict: Verdict,
    pub url: String,
    pub required: bool,
    /// Seconds it ran (0: unknown).
    pub secs: u64,
    pub description: String,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Commit {
    pub oid: String,
    pub short: String,
    pub headline: String,
    pub date: u64,
}

/// A PR's entry in the merge queue.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Queued {
    /// 1: next (0: unknown).
    pub position: u64,
    /// `QUEUED`, `AWAITING_CHECKS`, `MERGEABLE`, `UNMERGEABLE`, `LOCKED` ("": unknown).
    pub state: String,
    /// Estimated s until the merge (0: unknown).
    pub eta: u64,
}

/// Everything the merge tab shows of 1 PR.
#[derive(Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename = "Pr")]
pub(crate) struct PullRequest {
    pub pr: PrRef,
    pub title: String,
    pub body: String,
    pub url: String,
    pub state: State,
    pub author: String,
    pub created: u64,
    pub merged: Option<u64>,
    pub closed: Option<u64>,
    pub base: String,
    pub head: String,
    pub head_oid: String,
    pub additions: u64,
    pub deletions: u64,
    pub files: u64,
    /// `MERGEABLE`, `CONFLICTING`, `UNKNOWN`.
    pub mergeable: String,
    /// `CLEAN`, `BEHIND`, `BLOCKED`, `DIRTY`, `UNSTABLE`, `HAS_HOOKS`, `DRAFT`, `UNKNOWN`.
    pub merge_state: String,
    /// `APPROVED`, `CHANGES_REQUESTED`, `REVIEW_REQUIRED`, "".
    pub review: String,
    pub can_edit: bool,
    /// Auto-merge on: its method.
    pub auto_merge: Option<String>,
    /// The PR's node id (GraphQL mutations).
    #[serde(default)]
    pub id: String,
    /// The repo allows auto-merge.
    #[serde(default)]
    pub auto_allowed: bool,
    /// The base branch merges through a merge queue.
    #[serde(default)]
    pub queue: bool,
    /// In the merge queue: its entry.
    #[serde(default)]
    pub queued: Option<Queued>,
    /// The repo's merge method for the viewer (`MERGE`, `SQUASH`, `REBASE`).
    pub method: String,
    pub labels: Vec<Label>,
    /// Requested reviewers (users, teams).
    pub requested: Vec<String>,
    /// The latest verdict per reviewer: (login, state).
    pub reviewers: Vec<(String, String)>,
    /// The conversation, latest first.
    pub threads: Vec<Thread>,
    pub commits: Vec<Commit>,
    pub checks: Vec<Check>,
    pub viewer: String,
}

/// What blocks the merge, or 0 (ready).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Ready {
    Merged,
    Closed,
    Draft,
    Queued,
    Conflicts,
    ChecksFailed,
    ChecksRunning,
    ChangesRequested,
    NeedsApproval,
    Behind,
    Blocked,
    Mergeable,
    Unknown,
}

impl Ready {
    /// Title, detail.
    pub(crate) fn text(self) -> (&'static str, &'static str) {
        match self {
            Ready::Merged => ("merged", "the pull request is merged"),
            Ready::Closed => ("closed", "the pull request is closed"),
            Ready::Draft => ("draft", "a draft: mark it ready for review to merge"),
            Ready::Queued => ("in the merge queue", "GitHub merges it after the checks of the queue"),
            Ready::Conflicts => ("conflicts", "the branch has conflicts with the base"),
            Ready::ChecksFailed => ("checks failed", "1 or more checks failed"),
            Ready::ChecksRunning => ("checks running", "checks still run"),
            Ready::ChangesRequested => ("changes requested", "a reviewer requested changes"),
            Ready::NeedsApproval => ("needs approval", "a review is required"),
            Ready::Behind => ("not up to date", "the branch is behind the base"),
            Ready::Blocked => ("blocked", "a rule of the base branch blocks the merge"),
            Ready::Mergeable => ("ready to merge", "all requirements are met"),
            Ready::Unknown => ("checking", "GitHub computes the merge state"),
        }
    }
}

impl Pr {
    pub(crate) fn ready(&self) -> Ready {
        match self.state {
            State::Merged => return Ready::Merged,
            State::Closed => return Ready::Closed,
            State::Draft => return Ready::Draft,
            State::Open => {}
        }

        if self.queued.is_some() {
            return Ready::Queued;
        }

        if self.mergeable == "CONFLICTING" || self.merge_state == "DIRTY" {
            return Ready::Conflicts;
        }

        let required_fail = self
            .checks
            .iter()
            .any(|check| check.verdict == Verdict::Fail && check.required);
        let any_fail = self.checks.iter().any(|check| check.verdict == Verdict::Fail);

        if required_fail || (any_fail && self.merge_state == "BLOCKED") {
            return Ready::ChecksFailed;
        }

        if self.checks.iter().any(|check| check.verdict == Verdict::Pending)
            && matches!(self.merge_state.as_str(), "BLOCKED" | "UNSTABLE" | "UNKNOWN")
        {
            return Ready::ChecksRunning;
        }

        match self.review.as_str() {
            "CHANGES_REQUESTED" => return Ready::ChangesRequested,
            "REVIEW_REQUIRED" => return Ready::NeedsApproval,
            _ => {}
        }

        match self.merge_state.as_str() {
            "BEHIND" => Ready::Behind,
            "CLEAN" | "UNSTABLE" | "HAS_HOOKS" => Ready::Mergeable,
            "BLOCKED" => Ready::Blocked,
            _ => Ready::Unknown,
        }
    }

    /// The branch needs an update (behind the base, or conflicts).
    pub(crate) fn needs_update(&self) -> bool {
        matches!(self.state, State::Open | State::Draft)
            && self.queued.is_none()
            && (self.merge_state == "BEHIND" || self.mergeable == "CONFLICTING" || self.merge_state == "DIRTY")
    }

    pub(crate) fn conflicts(&self) -> bool {
        self.mergeable == "CONFLICTING" || self.merge_state == "DIRTY"
    }

    /// (failed, passed, skipped, pending).
    pub(crate) fn check_counts(&self) -> (usize, usize, usize, usize) {
        let count_of = |verdict: Verdict| self.checks.iter().filter(|check| check.verdict == verdict).count();

        (
            count_of(Verdict::Fail),
            count_of(Verdict::Pass),
            count_of(Verdict::Skip),
            count_of(Verdict::Pending),
        )
    }
}

/// ISO 8601 UTC (`2026-11-19T19:00:41Z`) → unix s (0: unparsable).
pub(crate) fn iso(text: &str) -> u64 {
    let bytes = text.as_bytes();

    if bytes.len() < 19 {
        return 0;
    }

    let field = |range: std::ops::Range<usize>| text.get(range).and_then(|part| part.parse::<i64>().ok());

    let (Some(y), Some(month), Some(day), Some(hour), Some(minute), Some(second)) = (
        field(0..4),
        field(5..7),
        field(8..10),
        field(11..13),
        field(14..16),
        field(17..19),
    ) else {
        return 0;
    };

    // days from civil (H. Hinnant)
    let y = if month <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;

    (days * 86400 + hour * 3600 + minute * 60 + second).max(0) as u64
}

/// Unix s → local `2026-11-19 20:00:41`.
pub(crate) fn stamp(ts: u64) -> String {
    if ts == 0 {
        return "?".into();
    }

    let secs = ts as libc::time_t;

    // SAFETY: localtime_r writes only into TM.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };

    unsafe { libc::localtime_r(&raw const secs, &raw mut tm) };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    )
}

const COMMENT: &str = "id databaseId author{login} body createdAt url viewerDidAuthor";

/// An edit of a PR.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Op {
    Title(String),
    Body(String),
    Close,
    Reopen,
    Ready,
    Draft,
    AddLabel(String),
    RemoveLabel(String),
    Comment(String),
    /// Reply to a line thread: its 1st comment's id.
    Reply(u64, String),
    Edit(Kind, u64, String),
    Delete(Kind, u64),
    /// METHOD (`MERGE` …), AUTO: when ready.
    Merge(String, bool),
    DisableAuto,
    /// Out of the merge queue: the PR's node id.
    Dequeue(String),
}

impl Op {
    /// What the toast says when it worked.
    pub(crate) fn done(&self) -> &'static str {
        match self {
            Op::Title(_) => "title saved",
            Op::Body(_) => "description saved",
            Op::Close => "pull request closed",
            Op::Reopen => "pull request reopened",
            Op::Ready => "marked ready for review",
            Op::Draft => "converted to draft",
            Op::AddLabel(_) => "label added",
            Op::RemoveLabel(_) => "label removed",
            Op::Comment(_) => "comment posted",
            Op::Reply(..) => "reply posted",
            Op::Edit(..) => "comment saved",
            Op::Delete(..) => "comment deleted",
            Op::Merge(_, true) => "auto-merge on: merges when ready",
            Op::Merge(_, false) => "merged",
            Op::DisableAuto => "auto-merge off",
            Op::Dequeue(_) => "out of the merge queue",
        }
    }
}

#[cfg(test)]
#[path = "gh_tests.rs"]
mod tests;

#[path = "gh_key.rs"]
mod key;

#[path = "gh_parse.rs"]
mod parse;
use parse::{nodes, parse_detail, parse_items};
#[path = "gh_requests.rs"]
mod requests;
pub(crate) use requests::{diff, fetch, for_branch, labels, mine, quote, run};

pub(crate) type Pr = PullRequest;

impl std::fmt::Debug for PullRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Pr")
            .field("pr", &self.pr)
            .field("title", &self.title)
            .field("body", &self.body)
            .field("url", &self.url)
            .field("state", &self.state)
            .field("author", &self.author)
            .field("created", &self.created)
            .field("merged", &self.merged)
            .field("closed", &self.closed)
            .field("base", &self.base)
            .field("head", &self.head)
            .field("head_oid", &self.head_oid)
            .field("additions", &self.additions)
            .field("deletions", &self.deletions)
            .field("files", &self.files)
            .field("mergeable", &self.mergeable)
            .field("merge_state", &self.merge_state)
            .field("review", &self.review)
            .field("can_edit", &self.can_edit)
            .field("auto_merge", &self.auto_merge)
            .field("id", &self.id)
            .field("auto_allowed", &self.auto_allowed)
            .field("queue", &self.queue)
            .field("queued", &self.queued)
            .field("method", &self.method)
            .field("labels", &self.labels)
            .field("requested", &self.requested)
            .field("reviewers", &self.reviewers)
            .field("threads", &self.threads)
            .field("commits", &self.commits)
            .field("checks", &self.checks)
            .field("viewer", &self.viewer)
            .finish()
    }
}
