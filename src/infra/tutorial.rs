use crate::infra::gh::{Check, Comment, Commit, Kind as CKind, Label, Pr, State, Thread, Verdict};
use std::path::{Path, PathBuf};
/// The tutorial's sandbox repo, when this process runs in the tutorial ($`CODEHEAD_TUTORIAL)`:
/// 0 IDE opens, 0 browser opens, 0 GitHub call (a mock PR instead).
pub(crate) fn active() -> Option<PathBuf> {
    std::env::var_os("CODEHEAD_TUTORIAL")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// The mock PR's merge marker in REPO.
pub(crate) fn merged_marker(repo: &Path) -> PathBuf {
    repo.join(".git/codehead-tutorial-merged")
}

/// The mock PR is merged.
pub(crate) fn mock_merged(repo: &Path) -> bool {
    merged_marker(repo).exists()
}

/// git's output in REPO ("" on a fail).
pub(crate) fn git_out(repo: &Path, args: &[&str]) -> String {
    std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default()
}

/// The 1st commit of REPO (the PR's base).
pub(crate) fn root_commit(repo: &Path) -> String {
    git_out(repo, &["rev-list", "--max-parents=0", "HEAD"])
        .lines()
        .next()
        .unwrap_or_default()
        .to_string()
}

/// The mock PR R: the owner's commits of REPO since its 1st, checks green, the reviewer's ok.
pub(crate) fn mock_pr(pr_ref: &crate::infra::gh::PrRef, repo: &Path) -> crate::infra::gh::Pr {
    let base = root_commit(repo);
    let range = format!("{base}..HEAD");

    let commits: Vec<Commit> = git_out(repo, &["log", "--reverse", "--format=%H%x09%h%x09%ct%x09%s", &range])
        .lines()
        .filter_map(|line| {
            let mut it = line.splitn(4, '\t');
            let (oid, short, at, headline) = (it.next()?, it.next()?, it.next()?, it.next()?);

            Some(Commit {
                oid: oid.into(),
                short: short.into(),
                date: at.parse::<u64>().unwrap_or(0),
                headline: headline.into(),
            })
        })
        .collect();

    let (mut adds, mut dels, mut files) = (0, 0, 0);

    for line in git_out(repo, &["diff", "--numstat", &base, "HEAD"]).lines() {
        let mut it = line.split_whitespace();

        adds += it.next().and_then(|number| number.parse::<u64>().ok()).unwrap_or(0);
        dels += it.next().and_then(|number| number.parse::<u64>().ok()).unwrap_or(0);
        files += 1;
    }

    let reviewer = std::fs::read_to_string(repo.join(".git/codehead-tutorial-reviewer"))
        .ok()
        .map(|name| name.trim().to_lowercase())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "crew".into());
    let merged = mock_merged(repo);
    let now = crate::engine::now_ms() / 1000;
    let created = commits.last().map_or(now, |commit| commit.date);
    let check = |name: &str, secs| Check {
        name: name.into(),
        group: "CI".into(),
        verdict: Verdict::Pass,
        url: String::new(),
        required: true,
        secs,
        description: String::new(),
    };

    build_mock_pr(
        pr_ref,
        commits,
        reviewer,
        merged,
        check,
        PrStats {
            adds,
            dels,
            files,
            now,
            created,
        },
    )
}

fn build_mock_pr(
    pr_ref: &super::gh::PrRef,
    commits: Vec<super::gh::Commit>,
    reviewer: String,
    merged: bool,
    check: impl Fn(&str, u64) -> super::gh::Check,
    settings: PrStats,
) -> super::gh::Pr {
    let PrStats {
        adds,
        dels,
        files,
        now,
        created,
    } = settings;

    Pr {
        pr: pr_ref.clone(),
        title: commits.first().map_or_else(|| "make hello.md say hi to the crew".into(), |commit| commit.headline.clone()),
        body: "hello.md greets the crew, in the style of the guide.\n\n- lower case, 1 line per idea\n- reviewed by the crew\n\n(the tutorial's mock PR: 0 GitHub calls)".into(),
        url: format!("https://github.com/{}/pull/{}", pr_ref.repo, pr_ref.number),
        state: if merged { State::Merged } else { State::Open },
        author: "you".into(),
        viewer: "you".into(),
        created,
        merged: merged.then_some(now),
        base: "main".into(),
        head: "task/hello".into(),
        head_oid: commits.last().map(|commit| commit.oid.clone()).unwrap_or_default(),
        additions: adds,
        deletions: dels,
        files,
        mergeable: "MERGEABLE".into(),
        merge_state: "CLEAN".into(),
        review: "APPROVED".into(),
        can_edit: true,
        method: "SQUASH".into(),
        labels: vec![Label { name: "docs".into(), color: "0075ca".into() }],
        reviewers: vec![(reviewer.clone(), "APPROVED".into())],
        threads: vec![Thread { root: Comment { kind: CKind::Review, id: 1, author: reviewer, body: "approved. 0 more nits.".into(), created, verdict: "APPROVED".into(), ..Default::default() }, ..Default::default() }],
        commits,
        checks: vec![check("lint", 14), check("test", 41)],
        ..Default::default()
    }
}

/// The mock PR's diff: COMMIT's own, else every commit since the 1st.
pub(crate) fn mock_diff(repo: &Path, commit: Option<&str>) -> String {
    match commit {
        Some(sha) => git_out(repo, &["show", "--format=", sha]),
        None => git_out(repo, &["diff", &root_commit(repo), "HEAD"]),
    }
}

/// An edit of the mock PR: a merge marks it merged; the rest: as if done.
pub(crate) fn mock_run(repo: &Path, op: &crate::infra::gh::Op) -> Result<(), String> {
    if let crate::infra::gh::Op::Merge(..) = op {
        std::fs::write(merged_marker(repo), "").map_err(|error| error.to_string())?;
    }

    Ok(())
}

#[derive(Clone, Copy)]
struct PrStats {
    adds: u64,
    dels: u64,
    files: u64,
    now: u64,
    created: u64,
}
