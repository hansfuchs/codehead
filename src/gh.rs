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
pub struct PrRef {
    pub repo: String,
    pub number: u64,
}

impl PrRef {
    pub fn new(repo: &str, number: u64) -> PrRef {
        PrRef { repo: repo.to_string(), number }
    }

    /// The tab label: `name#123`.
    pub fn label(&self) -> String {
        format!("{}#{}", self.repo.rsplit('/').next().unwrap_or(&self.repo), self.number)
    }

    fn owner_name(&self) -> (&str, &str) {
        self.repo.split_once('/').unwrap_or(("", &self.repo))
    }

    /// `owner/name/123` (a key for files).
    pub fn key(&self) -> String {
        format!("{}/{}", self.repo, self.number)
    }

}

/// Every PR URL in TEXT (`github.com/owner/name/pull/123`), in order, 0 dupes.
pub fn pr_urls(text: &str) -> Vec<PrRef> {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| regex::Regex::new(r"github\.com/([A-Za-z0-9_.-]+)/([A-Za-z0-9_.-]+)/pull/([0-9]+)").expect("valid"));
    let mut out: Vec<PrRef> = Vec::new();
    for c in re.captures_iter(text) {
        let r = PrRef::new(&format!("{}/{}", &c[1], &c[2]), c[3].parse().unwrap_or(0));
        if r.number > 0 && !out.contains(&r) {
            out.push(r);
        }
    }
    out
}

/// `owner/name` of a git remote URL (ssh or https, github.com only).
pub fn repo_of_remote(url: &str) -> Option<String> {
    let u = url.trim().trim_end_matches('/').trim_end_matches(".git");
    let rest = u.strip_prefix("git@github.com:").or_else(|| u.split_once("github.com/").map(|(_, r)| r)).or_else(|| u.strip_prefix("ssh://git@github.com/"))?;
    let mut it = rest.split('/');
    let (o, n) = (it.next()?, it.next()?);
    (!o.is_empty() && !n.is_empty()).then(|| format!("{o}/{n}"))
}

/// `owner/name` of the checkout DIR (its `origin`, else its 1st GitHub remote).
pub fn repo_of_dir(dir: &Path) -> Option<String> {
    let git = |args: &[&str]| Command::new("git").arg("-C").arg(dir).args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
    if let Some(r) = git(&["remote", "get-url", "origin"]).and_then(|u| repo_of_remote(&u)) {
        return Some(r);
    }
    git(&["remote", "-v"])?.lines().find_map(|l| l.split_whitespace().nth(1).and_then(repo_of_remote))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum State {
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

    pub fn label(self) -> &'static str {
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
pub struct Item {
    pub pr: PrRef,
    pub title: String,
    pub state: State,
    /// Unix s.
    pub created: u64,
    /// The head branch.
    pub head: String,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Label {
    pub name: String,
    /// `rrggbb`.
    pub color: String,
}

/// Where a comment lives (the API to edit, delete, reply).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Kind {
    /// On the conversation (REST: issues/comments).
    #[default]
    Issue,
    /// The body of a review (REST: pulls/reviews; 0 delete).
    Review,
    /// On a diff line, in a thread (REST: pulls/comments).
    Line,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Comment {
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
pub struct Thread {
    pub root: Comment,
    pub replies: Vec<Comment>,
    pub path: Option<String>,
    pub line: Option<u32>,
    pub resolved: bool,
    pub outdated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub enum Verdict {
    Fail,
    Pending,
    Pass,
    Skip,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Check {
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
pub struct Commit {
    pub oid: String,
    pub short: String,
    pub headline: String,
    pub date: u64,
}

/// Everything the merge tab shows of 1 PR.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Pr {
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
pub enum Ready {
    Merged,
    Closed,
    Draft,
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
    pub fn text(self) -> (&'static str, &'static str) {
        match self {
            Ready::Merged => ("merged", "the pull request is merged"),
            Ready::Closed => ("closed", "the pull request is closed"),
            Ready::Draft => ("draft", "a draft: mark it ready for review to merge"),
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
    pub fn ready(&self) -> Ready {
        match self.state {
            State::Merged => return Ready::Merged,
            State::Closed => return Ready::Closed,
            State::Draft => return Ready::Draft,
            State::Open => {}
        }
        if self.mergeable == "CONFLICTING" || self.merge_state == "DIRTY" {
            return Ready::Conflicts;
        }
        let required_fail = self.checks.iter().any(|c| c.verdict == Verdict::Fail && c.required);
        let any_fail = self.checks.iter().any(|c| c.verdict == Verdict::Fail);
        if required_fail || (any_fail && self.merge_state == "BLOCKED") {
            return Ready::ChecksFailed;
        }
        if self.checks.iter().any(|c| c.verdict == Verdict::Pending) && matches!(self.merge_state.as_str(), "BLOCKED" | "UNSTABLE" | "UNKNOWN") {
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
    pub fn needs_update(&self) -> bool {
        matches!(self.state, State::Open | State::Draft) && (self.merge_state == "BEHIND" || self.mergeable == "CONFLICTING" || self.merge_state == "DIRTY")
    }

    pub fn conflicts(&self) -> bool {
        self.mergeable == "CONFLICTING" || self.merge_state == "DIRTY"
    }

    /// (failed, passed, skipped, pending).
    pub fn check_counts(&self) -> (usize, usize, usize, usize) {
        let n = |v: Verdict| self.checks.iter().filter(|c| c.verdict == v).count();
        (n(Verdict::Fail), n(Verdict::Pass), n(Verdict::Skip), n(Verdict::Pending))
    }

}

/// ISO 8601 UTC (`2026-11-19T19:00:41Z`) → unix s (0: unparsable).
pub fn iso(s: &str) -> u64 {
    let b = s.as_bytes();
    if b.len() < 19 {
        return 0;
    }
    let n = |r: std::ops::Range<usize>| s.get(r).and_then(|x| x.parse::<i64>().ok());
    let (Some(y), Some(mo), Some(d), Some(h), Some(mi), Some(se)) = (n(0..4), n(5..7), n(8..10), n(11..13), n(14..16), n(17..19)) else { return 0 };
    // days from civil (H. Hinnant)
    let y = if mo <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    (days * 86400 + h * 3600 + mi * 60 + se).max(0) as u64
}

/// Unix s → local `2026-11-19 20:00:41`.
pub fn stamp(ts: u64) -> String {
    if ts == 0 {
        return "?".into();
    }
    let secs = ts as libc::time_t;
    // SAFETY: localtime_r writes only into TM.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&secs, &mut tm) };
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday, tm.tm_hour, tm.tm_min, tm.tm_sec)
}

/// gh ARGS (STDIN piped in), its stdout. Err: its 1st error line.
fn gh(args: &[&str], stdin: Option<&str>) -> Result<String, String> {
    if cfg!(test) && std::env::var_os("CODEHEAD_GH_LIVE").is_none() {
        return Err("gh: 0 calls in tests".into());
    }
    let mut c = Command::new("gh").args(args).stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("gh: {e}"))?;
    if let (Some(text), Some(mut i)) = (stdin, c.stdin.take()) {
        let _ = i.write_all(text.as_bytes());
    }
    let o = c.wait_with_output().map_err(|e| format!("gh: {e}"))?;
    if o.status.success() {
        Ok(String::from_utf8_lossy(&o.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&o.stderr);
        Err(err.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("gh failed").to_string())
    }
}

fn graphql(query: &str, vars: &[(&str, Value)]) -> Result<Value, String> {
    let body = json!({"query": query, "variables": vars.iter().map(|(k, v)| (k.to_string(), v.clone())).collect::<serde_json::Map<_, _>>()});
    let out = gh(&["api", "graphql", "--input", "-"], Some(&body.to_string()))?;
    let v: Value = serde_json::from_str(&out).map_err(|e| format!("gh: {e}"))?;
    if let Some(e) = v["errors"].as_array().and_then(|e| e.first()) {
        return Err(e["message"].as_str().unwrap_or("GraphQL error").to_string());
    }
    Ok(v["data"].clone())
}

const COMMENT: &str = "id databaseId author{login} body createdAt url viewerDidAuthor";

fn detail_query() -> String {
    format!(
        "query($o:String!,$r:String!,$n:Int!){{viewer{{login}} repository(owner:$o,name:$r){{viewerDefaultMergeMethod pullRequest(number:$n){{
number title body url state isDraft createdAt mergedAt closedAt author{{login}} baseRefName headRefName headRefOid additions deletions changedFiles
mergeable mergeStateStatus reviewDecision viewerCanUpdate autoMergeRequest{{mergeMethod}}
labels(first:50){{nodes{{name color}}}}
reviewRequests(first:20){{nodes{{requestedReviewer{{__typename ... on User{{login}} ... on Team{{name}} ... on Bot{{login}}}}}}}}
latestOpinionatedReviews(first:20){{nodes{{author{{login}} state}}}}
comments(last:100){{nodes{{{COMMENT}}}}}
reviews(last:50){{nodes{{{COMMENT} state}}}}
reviewThreads(first:100){{nodes{{isResolved isOutdated path line originalLine comments(first:50){{nodes{{{COMMENT}}}}}}}}}
commits(last:100){{nodes{{commit{{oid abbreviatedOid messageHeadline committedDate}}}}}}
last:commits(last:1){{nodes{{commit{{statusCheckRollup{{contexts(first:100){{nodes{{__typename
... on CheckRun{{name status conclusion detailsUrl startedAt completedAt isRequired(pullRequestNumber:$n) title checkSuite{{app{{name}} workflowRun{{workflow{{name}}}}}}}}
... on StatusContext{{context state targetUrl description isRequired(pullRequestNumber:$n)}}
}}}}}}}}}}}}}}}}}}"
    )
}

/// Everything of PR R (1 GraphQL call).
pub fn fetch(r: &PrRef) -> Result<Pr, String> {
    let (o, n) = r.owner_name();
    let d = graphql(&detail_query(), &[("o", json!(o)), ("r", json!(n)), ("n", json!(r.number))])?;
    parse_detail(r, &d).ok_or_else(|| format!("{}: 0 such pull request", r.label()))
}

fn comment(v: &Value, kind: Kind) -> Comment {
    Comment {
        kind,
        id: v["databaseId"].as_u64().unwrap_or(0),
        author: v["author"]["login"].as_str().unwrap_or("ghost").to_string(),
        body: v["body"].as_str().unwrap_or_default().replace("\r\n", "\n"),
        created: iso(v["createdAt"].as_str().unwrap_or_default()),
        url: v["url"].as_str().unwrap_or_default().to_string(),
        mine: v["viewerDidAuthor"].as_bool().unwrap_or(false),
        verdict: v["state"].as_str().unwrap_or_default().to_string(),
    }
}

fn nodes(v: &Value) -> impl Iterator<Item = &Value> {
    v["nodes"].as_array().into_iter().flatten()
}

fn parse_detail(r: &PrRef, d: &Value) -> Option<Pr> {
    let repo = &d["repository"];
    let p = &repo["pullRequest"];
    p["number"].as_u64()?;
    let s = |v: &Value| v.as_str().unwrap_or_default().to_string();
    let t = |v: &Value| v.as_str().map(iso).filter(|&x| x > 0);
    let mut threads: Vec<Thread> = nodes(&p["comments"]).map(|c| Thread { root: comment(c, Kind::Issue), ..Default::default() }).collect();
    // reviews with a body (0 body: only a verdict, or the review of a line thread)
    threads.extend(nodes(&p["reviews"]).filter(|c| !c["body"].as_str().unwrap_or_default().trim().is_empty()).map(|c| Thread { root: comment(c, Kind::Review), ..Default::default() }));
    for th in nodes(&p["reviewThreads"]) {
        let mut cs = nodes(&th["comments"]).map(|c| comment(c, Kind::Line));
        let Some(root) = cs.next() else { continue };
        threads.push(Thread {
            root,
            replies: cs.collect(),
            path: th["path"].as_str().map(String::from),
            line: th["line"].as_u64().or_else(|| th["originalLine"].as_u64()).map(|l| l as u32),
            resolved: th["isResolved"].as_bool().unwrap_or(false),
            outdated: th["isOutdated"].as_bool().unwrap_or(false),
        });
    }
    threads.sort_by_key(|t| std::cmp::Reverse(t.root.created));
    let mut checks = Vec::new();
    for c in nodes(&p["last"]).flat_map(|n| nodes(&n["commit"]["statusCheckRollup"]["contexts"]).cloned().collect::<Vec<_>>()) {
        let check = if c["__typename"] == "StatusContext" {
            Check {
                name: s(&c["context"]),
                group: String::new(),
                verdict: match c["state"].as_str().unwrap_or_default() {
                    "SUCCESS" => Verdict::Pass,
                    "FAILURE" | "ERROR" => Verdict::Fail,
                    _ => Verdict::Pending,
                },
                url: s(&c["targetUrl"]),
                required: c["isRequired"].as_bool().unwrap_or(false),
                secs: 0,
                description: s(&c["description"]),
            }
        } else {
            let verdict = match (c["status"].as_str().unwrap_or_default(), c["conclusion"].as_str().unwrap_or_default()) {
                ("COMPLETED", "SUCCESS" | "NEUTRAL") => Verdict::Pass,
                ("COMPLETED", "SKIPPED" | "STALE") => Verdict::Skip,
                ("COMPLETED", _) => Verdict::Fail,
                _ => Verdict::Pending,
            };
            let (a, b) = (t(&c["startedAt"]).unwrap_or(0), t(&c["completedAt"]).unwrap_or(0));
            let wf = c["checkSuite"]["workflowRun"]["workflow"]["name"].as_str().or_else(|| c["checkSuite"]["app"]["name"].as_str()).unwrap_or_default();
            Check { name: s(&c["name"]), group: wf.to_string(), verdict, url: s(&c["detailsUrl"]), required: c["isRequired"].as_bool().unwrap_or(false), secs: b.saturating_sub(a), description: s(&c["title"]) }
        };
        checks.push(check);
    }
    // failed first, then pending, passed, skipped; A→Z inside
    checks.sort_by(|a, b| a.verdict.cmp(&b.verdict).then_with(|| (&a.group, &a.name).cmp(&(&b.group, &b.name))));
    Some(Pr {
        pr: r.clone(),
        title: s(&p["title"]),
        body: s(&p["body"]).replace("\r\n", "\n"),
        url: s(&p["url"]),
        state: State::of(p["state"].as_str().unwrap_or_default(), p["isDraft"].as_bool().unwrap_or(false)),
        author: p["author"]["login"].as_str().unwrap_or("ghost").to_string(),
        created: t(&p["createdAt"]).unwrap_or(0),
        merged: t(&p["mergedAt"]),
        closed: t(&p["closedAt"]),
        base: s(&p["baseRefName"]),
        head: s(&p["headRefName"]),
        head_oid: s(&p["headRefOid"]),
        additions: p["additions"].as_u64().unwrap_or(0),
        deletions: p["deletions"].as_u64().unwrap_or(0),
        files: p["changedFiles"].as_u64().unwrap_or(0),
        mergeable: s(&p["mergeable"]),
        merge_state: s(&p["mergeStateStatus"]),
        review: s(&p["reviewDecision"]),
        can_edit: p["viewerCanUpdate"].as_bool().unwrap_or(false),
        auto_merge: p["autoMergeRequest"]["mergeMethod"].as_str().map(String::from),
        method: repo["viewerDefaultMergeMethod"].as_str().unwrap_or("MERGE").to_string(),
        labels: nodes(&p["labels"]).map(|l| Label { name: s(&l["name"]), color: s(&l["color"]) }).collect(),
        requested: nodes(&p["reviewRequests"]).filter_map(|n| n["requestedReviewer"]["login"].as_str().or_else(|| n["requestedReviewer"]["name"].as_str()).map(String::from)).collect(),
        reviewers: nodes(&p["latestOpinionatedReviews"]).map(|n| (n["author"]["login"].as_str().unwrap_or("ghost").to_string(), s(&n["state"]))).collect(),
        threads,
        commits: nodes(&p["commits"])
            .map(|n| {
                let c = &n["commit"];
                Commit { oid: s(&c["oid"]), short: s(&c["abbreviatedOid"]), headline: s(&c["messageHeadline"]), date: t(&c["committedDate"]).unwrap_or(0) }
            })
            .collect(),
        checks,
        viewer: d["viewer"]["login"].as_str().unwrap_or_default().to_string(),
    })
}

/// The viewer's PRs (every repo, every state), newest first: max. PAGES x 100.
pub fn mine(pages: usize) -> Result<Vec<Item>, String> {
    let q = "query($q:String!,$after:String){search(query:$q,type:ISSUE,first:100,after:$after){pageInfo{hasNextPage endCursor} nodes{... on PullRequest{number title state isDraft createdAt headRefName repository{nameWithOwner}}}}}";
    let mut out = Vec::new();
    let mut after = Value::Null;
    for _ in 0..pages.max(1) {
        let d = graphql(q, &[("q", json!("author:@me is:pr sort:created-desc")), ("after", after.clone())])?;
        out.extend(parse_items(&d["search"]));
        let pi = &d["search"]["pageInfo"];
        if pi["hasNextPage"] != true {
            break;
        }
        after = pi["endCursor"].clone();
    }
    Ok(out)
}

fn parse_items(v: &Value) -> Vec<Item> {
    nodes(v)
        .filter_map(|n| {
            Some(Item {
                pr: PrRef::new(n["repository"]["nameWithOwner"].as_str()?, n["number"].as_u64()?),
                title: n["title"].as_str().unwrap_or_default().to_string(),
                state: State::of(n["state"].as_str().unwrap_or_default(), n["isDraft"].as_bool().unwrap_or(false)),
                created: iso(n["createdAt"].as_str().unwrap_or_default()),
                head: n["headRefName"].as_str().unwrap_or_default().to_string(),
            })
        })
        .collect()
}

/// The PRs of BRANCH (as head) in REPO, every state.
pub fn for_branch(repo: &str, branch: &str) -> Result<Vec<u64>, String> {
    let (o, n) = repo.split_once('/').ok_or("0 owner")?;
    let q = "query($o:String!,$r:String!,$b:String!){repository(owner:$o,name:$r){pullRequests(headRefName:$b,first:10,orderBy:{field:CREATED_AT,direction:DESC}){nodes{number}}}}";
    let d = graphql(q, &[("o", json!(o)), ("r", json!(n)), ("b", json!(branch))])?;
    Ok(nodes(&d["repository"]["pullRequests"]).filter_map(|n| n["number"].as_u64()).collect())
}

/// The labels of REPO.
pub fn labels(repo: &str) -> Result<Vec<Label>, String> {
    let out = gh(&["label", "list", "-R", repo, "--limit", "500", "--json", "name,color"], None)?;
    let v: Value = serde_json::from_str(&out).map_err(|e| e.to_string())?;
    Ok(v.as_array().into_iter().flatten().map(|l| Label { name: l["name"].as_str().unwrap_or_default().into(), color: l["color"].as_str().unwrap_or_default().into() }).collect())
}

/// The diff of R: COMMIT's own, else the whole PR against the merge base.
pub fn diff(r: &PrRef, commit: Option<&str>) -> Result<String, String> {
    match commit {
        Some(c) => gh(&["api", &format!("repos/{}/commits/{c}", r.repo), "-H", "Accept: application/vnd.github.diff"], None),
        None => gh(&["pr", "diff", &r.number.to_string(), "-R", &r.repo], None),
    }
}

/// An edit of a PR.
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
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
}

impl Op {
    /// What the toast says when it worked.
    pub fn done(&self) -> &'static str {
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
        }
    }
}

/// Run OP on R.
pub fn run(r: &PrRef, op: &Op) -> Result<(), String> {
    let n = r.number.to_string();
    let pr = |args: &[&str], stdin: Option<&str>| -> Result<(), String> {
        let mut a = vec!["pr"];
        a.extend_from_slice(args);
        a.extend(["-R", r.repo.as_str()]);
        gh(&a, stdin).map(|_| ())
    };
    let api = |method: &str, path: String, body: Option<Value>| -> Result<(), String> {
        let mut a = vec!["api", "-X", method, path.as_str()];
        let text = body.map(|b| b.to_string());
        if text.is_some() {
            a.extend(["--input", "-"]);
        }
        gh(&a, text.as_deref()).map(|_| ())
    };
    match op {
        Op::Title(t) => pr(&["edit", &n, "--title", t], None),
        Op::Body(b) => pr(&["edit", &n, "--body-file", "-"], Some(b)),
        Op::Close => pr(&["close", &n], None),
        Op::Reopen => pr(&["reopen", &n], None),
        Op::Ready => pr(&["ready", &n], None),
        Op::Draft => pr(&["ready", &n, "--undo"], None),
        Op::AddLabel(l) => pr(&["edit", &n, "--add-label", l], None),
        Op::RemoveLabel(l) => pr(&["edit", &n, "--remove-label", l], None),
        Op::Comment(b) => pr(&["comment", &n, "--body-file", "-"], Some(b)),
        Op::Reply(id, b) => api("POST", format!("repos/{}/pulls/{n}/comments/{id}/replies", r.repo), Some(json!({"body": b}))),
        Op::Edit(Kind::Issue, id, b) => api("PATCH", format!("repos/{}/issues/comments/{id}", r.repo), Some(json!({"body": b}))),
        Op::Edit(Kind::Line, id, b) => api("PATCH", format!("repos/{}/pulls/comments/{id}", r.repo), Some(json!({"body": b}))),
        Op::Edit(Kind::Review, id, b) => api("PUT", format!("repos/{}/pulls/{n}/reviews/{id}", r.repo), Some(json!({"body": b}))),
        Op::Delete(Kind::Issue, id) => api("DELETE", format!("repos/{}/issues/comments/{id}", r.repo), None),
        Op::Delete(Kind::Line, id) => api("DELETE", format!("repos/{}/pulls/comments/{id}", r.repo), None),
        Op::Delete(Kind::Review, _) => Err("a review body cannot be deleted: edit it".into()),
        Op::Merge(m, auto) => {
            let flag = match m.as_str() {
                "SQUASH" => "--squash",
                "REBASE" => "--rebase",
                _ => "--merge",
            };
            let mut a = vec!["merge", n.as_str(), flag];
            if *auto {
                a.push("--auto");
            }
            pr(&a, None)
        }
        Op::DisableAuto => pr(&["merge", &n, "--disable-auto"], None),
    }
}

/// Quote BODY as a markdown reply to AUTHOR.
pub fn quote(author: &str, body: &str) -> String {
    let q: Vec<String> = body.lines().map(|l| if l.is_empty() { ">".to_string() } else { format!("> {l}") }).collect();
    format!("@{author} wrote:\n{}\n\n", q.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refs_parse() {
        assert_eq!(PrRef::new("acme/platform", 1234).label(), "platform#1234");
        assert_eq!(pr_urls("see https://github.com/acme/platform/pull/12 and github.com/acme/web/pull/3, again https://github.com/acme/platform/pull/12/files"), vec![PrRef::new("acme/platform", 12), PrRef::new("acme/web", 3)]);
        assert_eq!(repo_of_remote("git@github.com:acme/platform.git").as_deref(), Some("acme/platform"));
        assert_eq!(repo_of_remote("https://github.com/acme/platform").as_deref(), Some("acme/platform"));
        assert_eq!(repo_of_remote("ssh://git@github.com/acme/platform.git").as_deref(), Some("acme/platform"));
        assert_eq!(repo_of_remote("git@gitlab.com:acme/platform.git"), None);
        assert_eq!(PrRef::new("a/b", 7).key(), "a/b/7");
    }

    #[test]
    fn iso_times() {
        assert_eq!(iso("1970-01-01T00:00:00Z"), 0);
        assert_eq!(iso("2026-11-19T19:00:41Z"), 1795114841);
        assert_eq!(iso("2000-02-29T12:00:00Z"), 951825600);
        assert_eq!(iso("bad"), 0);
    }

    fn fixture() -> Value {
        json!({
            "viewer": {"login": "me"},
            "repository": {"viewerDefaultMergeMethod": "SQUASH", "pullRequest": {
                "number": 7, "title": "Add X", "body": "body\r\nline", "url": "https://github.com/a/b/pull/7", "state": "OPEN", "isDraft": false,
                "createdAt": "2026-11-19T19:00:41Z", "mergedAt": null, "closedAt": null, "author": {"login": "me"},
                "baseRefName": "main", "headRefName": "feat/x", "headRefOid": "abc", "additions": 10, "deletions": 2, "changedFiles": 3,
                "mergeable": "MERGEABLE", "mergeStateStatus": "BLOCKED", "reviewDecision": "REVIEW_REQUIRED", "viewerCanUpdate": true, "autoMergeRequest": null,
                "labels": {"nodes": [{"name": "bug", "color": "d73a4a"}]},
                "reviewRequests": {"nodes": [{"requestedReviewer": {"__typename": "User", "login": "rev"}}]},
                "latestOpinionatedReviews": {"nodes": []},
                "comments": {"nodes": [{"databaseId": 1, "author": {"login": "rev"}, "body": "old", "createdAt": "2026-11-19T19:01:00Z", "url": "u1", "viewerDidAuthor": false}]},
                "reviews": {"nodes": [{"databaseId": 2, "author": {"login": "rev"}, "body": "", "createdAt": "2026-11-19T19:02:00Z", "url": "u2", "viewerDidAuthor": false, "state": "COMMENTED"}]},
                "reviewThreads": {"nodes": [{"isResolved": false, "isOutdated": false, "path": "src/x.rs", "line": 4, "originalLine": 3, "comments": {"nodes": [
                    {"databaseId": 3, "author": {"login": "rev"}, "body": "why?", "createdAt": "2026-11-19T19:03:00Z", "url": "u3", "viewerDidAuthor": false},
                    {"databaseId": 4, "author": {"login": "me"}, "body": "because", "createdAt": "2026-11-19T19:04:00Z", "url": "u4", "viewerDidAuthor": true}]}}]},
                "commits": {"nodes": [{"commit": {"oid": "abc", "abbreviatedOid": "abc", "messageHeadline": "x", "committedDate": "2026-11-19T19:00:00Z"}}]},
                "last": {"nodes": [{"commit": {"statusCheckRollup": {"contexts": {"nodes": [
                    {"__typename": "CheckRun", "name": "test", "status": "COMPLETED", "conclusion": "SUCCESS", "detailsUrl": "d1", "startedAt": "2026-11-19T19:00:00Z", "completedAt": "2026-11-19T19:00:27Z", "isRequired": true, "title": null, "checkSuite": {"app": {"name": "GitHub Actions"}, "workflowRun": {"workflow": {"name": "CI"}}}},
                    {"__typename": "CheckRun", "name": "lint", "status": "COMPLETED", "conclusion": "FAILURE", "detailsUrl": "d2", "startedAt": null, "completedAt": null, "isRequired": false, "title": null, "checkSuite": {"app": {"name": "GitHub Actions"}, "workflowRun": null}},
                    {"__typename": "CheckRun", "name": "deploy", "status": "COMPLETED", "conclusion": "SKIPPED", "detailsUrl": "d3", "isRequired": false, "checkSuite": {}},
                    {"__typename": "StatusContext", "context": "ci/ext", "state": "PENDING", "targetUrl": "d4", "description": "waiting", "isRequired": false}
                ]}}}}]}
            }}
        })
    }

    #[test]
    fn a_pr_parses() {
        let p = parse_detail(&PrRef::new("a/b", 7), &fixture()).unwrap();
        assert_eq!((p.title.as_str(), p.body.as_str(), p.state, p.method.as_str()), ("Add X", "body\nline", State::Open, "SQUASH"));
        assert_eq!(p.labels, vec![Label { name: "bug".into(), color: "d73a4a".into() }]);
        assert_eq!(p.requested, vec!["rev"]);
        // the review without a body: 0 thread; the line thread: its reply inside; latest first
        assert_eq!(p.threads.len(), 2);
        assert_eq!(p.threads[0].root.kind, Kind::Line);
        assert_eq!(p.threads[0].replies.len(), 1);
        assert!(p.threads[0].replies[0].mine);
        assert_eq!(p.threads[0].line, Some(4));
        assert_eq!(p.threads[0].path.as_deref(), Some("src/x.rs"));
        // failed first, pending, passed, skipped
        let v: Vec<Verdict> = p.checks.iter().map(|c| c.verdict).collect();
        assert_eq!(v, vec![Verdict::Fail, Verdict::Pending, Verdict::Pass, Verdict::Skip]);
        assert_eq!(p.checks[2].secs, 27);
        assert_eq!(p.check_counts(), (1, 1, 1, 1));
        // a failed check (not required) on a blocked PR → checks failed
        assert_eq!(p.ready(), Ready::ChecksFailed);
    }

    #[test]
    fn readiness() {
        let mut p = parse_detail(&PrRef::new("a/b", 7), &fixture()).unwrap();
        p.checks.retain(|c| c.verdict == Verdict::Pass);
        assert_eq!(p.ready(), Ready::NeedsApproval);
        p.review = "APPROVED".into();
        p.merge_state = "BEHIND".into();
        assert_eq!(p.ready(), Ready::Behind);
        assert!(p.needs_update() && !p.conflicts());
        p.merge_state = "CLEAN".into();
        assert_eq!(p.ready(), Ready::Mergeable);
        p.mergeable = "CONFLICTING".into();
        assert_eq!(p.ready(), Ready::Conflicts);
        p.state = State::Draft;
        assert_eq!(p.ready(), Ready::Draft);
    }

    #[test]
    fn list_items_parse() {
        let v = json!({"nodes": [{"number": 3, "title": "t", "state": "MERGED", "isDraft": false, "createdAt": "2026-01-01T00:00:00Z", "repository": {"nameWithOwner": "a/b"}}, {}]});
        let items = parse_items(&v);
        assert_eq!(items.len(), 1);
        assert_eq!((items[0].pr.label(), items[0].state), ("b#3".to_string(), State::Merged));
    }

    /// CODEHEAD_GH_LIVE=1 cargo test live_ -- --ignored: the queries against github.com.
    #[test]
    #[ignore]
    fn live_queries() {
        let p = fetch(&PrRef::new("cli/cli", 15100)).unwrap();
        assert!(!p.title.is_empty() && !p.checks.is_empty() && !p.threads.is_empty(), "{p:?}");
        assert!(!mine(1).unwrap().is_empty());
        assert!(diff(&p.pr, None).unwrap().starts_with("diff --git"));
        assert!(diff(&p.pr, Some(&p.commits[0].oid)).unwrap().contains("diff --git"));
        assert!(!labels("cli/cli").unwrap().is_empty());
        let _ = for_branch("cli/cli", &p.head).unwrap();
    }

    #[test]
    fn quotes() {
        assert_eq!(quote("x", "a\n\nb"), "@x wrote:\n> a\n>\n> b\n\n");
    }
}
