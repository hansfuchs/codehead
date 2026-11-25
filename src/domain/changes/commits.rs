//! The branch's commits for the panel's COMMITS section: unpushed (not on the upstream) and
//! pushed, newest first. Uncommit: picked unpushed commits leave the history, their changes stay
//! in the worktree as uncommitted changes (git plumbing: 0 worktree write); push up to a commit.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};

use super::git;

/// Commits listed at most (a long history: the newest ones).
pub(crate) const MAX: usize = 50;

/// A commit of the branch.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Commit {
    pub oid: String,
    pub short: String,
    pub subject: String,
    /// The whole message (subject + body).
    pub body: String,
    pub author: String,
    /// Commit time (unix seconds).
    pub time: i64,
    /// On the upstream (else: only local).
    pub pushed: bool,
    /// 2+ parents.
    pub merge: bool,
}

/// The branch's commits: newest first, the unpushed ones before the pushed ones.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Commits {
    pub list: Vec<Commit>,
    /// `origin/x`: the upstream (None: 0 upstream, every commit unpushed).
    pub upstream: Option<String>,
    /// Commits on the upstream, 0 on the branch (a pull / rebase needed before a push).
    pub remote_only: usize,
    /// More commits than `MAX`.
    pub more: bool,
    /// HEAD's oid at the read (an uncommit refuses when HEAD moved since).
    pub head: String,
}

impl Commits {
    pub(crate) fn find(&self, oid: &str) -> Option<&Commit> {
        self.list.iter().find(|commit| commit.oid == oid)
    }
}

/// DIR's branch (None: detached HEAD).
fn branch(dir: &Path) -> Option<String> {
    let branch = git(dir, &["rev-parse", "--abbrev-ref", "HEAD"])
        .ok()?
        .trim()
        .to_string();

    (!branch.is_empty() && branch != "HEAD").then_some(branch)
}

/// The remote branch B of DIR tracks when that remote branch has B's name (`refs/heads/B`).
fn tracked_remote(dir: &Path, branch: &str) -> Option<String> {
    let out = git(
        dir,
        &[
            "for-each-ref",
            "--format=%(upstream:remotename)%00%(upstream:remoteref)",
            &format!("refs/heads/{branch}"),
        ],
    )
    .ok()?;
    let (remote, rref) = out.trim().split_once('\0')?;

    (!remote.is_empty() && rref == format!("refs/heads/{branch}")).then(|| remote.to_string())
}

/// The upstream of DIR's branch `x`: the tracked branch when its name is `x` too (`origin/x`,
/// `fork/x`), else `origin/x` when it exists. A branch that tracks another name (`origin/main`,
/// `origin/hans/x`): `origin/x`, so a push never lands on that other branch.
pub(crate) fn upstream(dir: &Path) -> Option<String> {
    let branch = branch(dir)?;

    // a remote branch deleted after the merge (`[gone]`): 0 upstream, every commit unpushed
    let exists = |remote_branch: &str| {
        git(
            dir,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("refs/remotes/{remote_branch}"),
            ],
        )
        .is_ok()
    };

    if let Some(tracked) = tracked_remote(dir, &branch).map(|remote| format!("{remote}/{branch}"))
        && exists(&tracked)
    {
        return Some(tracked);
    }

    let origin = format!("origin/{branch}");

    exists(&origin).then_some(origin)
}

/// The commits of DIR's branch since REV (the merge-base; `HEAD`: 0 base, the newest `MAX`).
pub(crate) fn list(dir: &Path, rev: &str) -> Commits {
    let up = upstream(dir);
    let limit = format!("-n{}", MAX + 1);
    let mut args = vec![
        "log",
        "--topo-order",
        &limit,
        "--format=%H%x1f%h%x1f%P%x1f%an%x1f%ct%x1f%s%x1f%B%x1e",
        "HEAD",
    ];
    let not = format!("^{rev}");

    if rev != "HEAD" && !rev.is_empty() {
        args.push(&not);
    }

    let Ok(out) = git(dir, &args) else {
        return Commits {
            upstream: up,
            ..Default::default()
        };
    };

    // unpushed: on HEAD, 0 on the upstream (`<`; 0 upstream: all); `>`: on the upstream only
    let mut remote_only = 0;

    // a failed read: unknown, as 0 upstream (never "all pushed")
    let local: Option<HashSet<String>> = up.as_ref().and_then(|upstream| {
        let lr = git(dir, &["rev-list", "--left-right", &format!("HEAD...{upstream}")]).ok()?;

        remote_only = lr.lines().filter(|line| line.starts_with('>')).count();
        Some(
            lr.lines()
                .filter_map(|line| line.strip_prefix('<'))
                .map(str::to_string)
                .collect(),
        )
    });

    parse_commit_list(up, &out, remote_only, local.as_ref())
}

fn parse_commit_list(up: Option<String>, out: &str, remote_only: usize, local: Option<&HashSet<String>>) -> Commits {
    let mut list: Vec<Commit> = out
        .split('\x1e')
        .filter_map(|rec| {
            let rec = rec.trim_start_matches('\n');
            let fields: Vec<&str> = rec.splitn(7, '\x1f').collect();

            if fields.len() < 7 {
                return None;
            }

            let pushed = local.as_ref().is_some_and(|ids| !ids.contains(fields[0]));

            Some(Commit {
                oid: fields[0].into(),
                short: fields[1].into(),
                merge: fields[2].split_whitespace().count() > 1,
                author: fields[3].into(),
                time: fields[4].parse().unwrap_or(0),
                subject: fields[5].into(),
                body: fields[6].trim_end().into(),
                pushed,
            })
        })
        .collect();

    // HEAD: the log's 1st commit (0 commits: 0 pick, 0 uncommit, HEAD unused)
    let head = list.first().map(|commit| commit.oid.clone()).unwrap_or_default();
    let more = list.len() > MAX;

    list.truncate(MAX);

    // unpushed first (stable: newest first within each)
    list.sort_by_key(|commit| commit.pushed);

    Commits {
        list,
        upstream: up,
        remote_only,
        more,
        head,
    }
}

/// `git ARGS` in DIR with ENV and STDIN: Ok(stdout) / Err((exit code, stdout, 1st stderr line)).
fn run(
    dir: &Path,
    args: &[&str],
    env: &[(&str, String)],
    stdin: Option<&str>,
) -> Result<String, (i32, String, String)> {
    use std::io::Write;
    let mut command = Command::new("git");

    command
        .arg("-C")
        .arg(dir)
        .args(["-c", "core.quotePath=false", "-c", "log.showSignature=false"])
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() });

    for (key, value) in env {
        command.env(key, value);
    }

    let mut child = command
        .spawn()
        .map_err(|error| (-1, String::new(), format!("git: {error}")))?;

    if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
        let _ = pipe.write_all(text.as_bytes());
    }

    let out = child
        .wait_with_output()
        .map_err(|error| (-1, String::new(), format!("git: {error}")))?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();

    if out.status.success() {
        Ok(stdout)
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr);

        Err((
            out.status.code().unwrap_or(-1),
            stdout,
            stderr
                .lines()
                .find(|line| !line.trim().is_empty())
                .unwrap_or("git failed")
                .to_string(),
        ))
    }
}

fn short(oid: &str) -> &str {
    &oid[..oid.len().min(7)]
}

/// What an uncommit did.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Uncommitted {
    pub count: usize,
    /// A later commit's change of a file that conflicted: out of the history too (short, path).
    pub cascaded: Vec<(String, String)>,
    /// Later commits with 0 change left: dropped (short).
    pub dropped: Vec<String>,
    /// The staged changes did not fit the new HEAD: the index kept as it was.
    pub index_kept: bool,
}

impl Uncommitted {
    pub(crate) fn note(&self) -> String {
        let mut text = format!(
            "uncommitted {} commit{}",
            self.count,
            if self.count == 1 { "" } else { "s" }
        );

        if !self.cascaded.is_empty() {
            let parts: Vec<String> = self
                .cascaded
                .iter()
                .map(|(commit, path)| format!("{path} ({commit})"))
                .collect();

            let _ = write!(text, " · cascaded: {}", parts.join(", "));
        }

        if !self.dropped.is_empty() {
            let _ = write!(text, " · emptied, dropped: {}", self.dropped.join(", "));
        }

        if self.index_kept {
            text.push_str(" · index kept: the changes show staged");
        }

        text
    }
}

/// Push DIR's commits up to OID (OID and the unpushed commits before it) to the branch of the
/// same name (as `push origin HEAD`): on the upstream's remote, else on origin. Never to the
/// branch of another name the local one may track.
pub(crate) fn push_to(dir: &Path, oid: &str) -> Result<String, String> {
    let Some(branch) = branch(dir) else {
        return Err("detached HEAD: 0 branch to push to".into());
    };

    // an agent rebased while the question was open: the commit left the branch
    if git(dir, &["merge-base", "--is-ancestor", oid, "HEAD"]).is_err() {
        return Err(format!("{}: 0 more on the branch (read again: r)", short(oid)));
    }

    let tracked = tracked_remote(dir, &branch);
    let remote = tracked.clone().unwrap_or_else(|| "origin".into());
    let out = git(dir, &["push", &remote, &format!("{oid}:refs/heads/{branch}")])?;

    // 0 upstream of the same name yet: the pushed branch becomes it (as `push -u` does)
    if tracked.is_none() {
        let _ = git(dir, &["branch", &format!("--set-upstream-to={remote}/{branch}")]);
    }

    Ok(out)
}

/// Test repos: a work checkout with a bare `origin`.
#[cfg(test)]
#[path = "commits_fixture_tests.rs"]
pub(crate) mod fixture;

#[cfg(test)]
#[path = "commits_tests.rs"]
mod tests;

#[path = "commit_history.rs"]
mod commit_history;
pub(crate) use commit_history::*;
