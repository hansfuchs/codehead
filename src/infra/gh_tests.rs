use super::*;

#[test]
fn refs_parse() {
    assert_eq!(PrRef::new("acme/platform", 1234).label(), "platform#1234");
    assert_eq!(
        pr_urls(
            "see https://github.com/acme/platform/pull/12 and github.com/acme/web/pull/3, again https://github.com/acme/platform/pull/12/files"
        ),
        vec![PrRef::new("acme/platform", 12), PrRef::new("acme/web", 3)]
    );
    assert_eq!(
        repo_of_remote("git@github.com:acme/platform.git").as_deref(),
        Some("acme/platform")
    );
    assert_eq!(
        repo_of_remote("https://github.com/acme/platform").as_deref(),
        Some("acme/platform")
    );
    assert_eq!(
        repo_of_remote("ssh://git@github.com/acme/platform.git").as_deref(),
        Some("acme/platform")
    );
    assert_eq!(repo_of_remote("git@gitlab.com:acme/platform.git"), None);
    assert_eq!(PrRef::new("a/b", 7).key(), "a/b/7");
}

#[test]
fn iso_times() {
    assert_eq!(iso("1970-01-01T00:00:00Z"), 0);
    assert_eq!(iso("2026-11-19T19:00:41Z"), 1_795_114_841);
    assert_eq!(iso("2000-02-29T12:00:00Z"), 951_825_600);
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
    let pr = parse_detail(&PrRef::new("a/b", 7), &fixture()).unwrap();

    assert_eq!(
        (pr.title.as_str(), pr.body.as_str(), pr.state, pr.method.as_str()),
        ("Add X", "body\nline", State::Open, "SQUASH")
    );
    assert_eq!(
        pr.labels,
        vec![Label {
            name: "bug".into(),
            color: "d73a4a".into()
        }]
    );
    assert_eq!(pr.requested, vec!["rev"]);

    // the review without a body: 0 thread; the line thread: its reply inside; latest first
    assert_eq!(pr.threads.len(), 2);
    assert_eq!(pr.threads[0].root.kind, Kind::Line);
    assert_eq!(pr.threads[0].replies.len(), 1);
    assert!(pr.threads[0].replies[0].mine);
    assert_eq!(pr.threads[0].line, Some(4));
    assert_eq!(pr.threads[0].path.as_deref(), Some("src/x.rs"));

    // failed first, pending, passed, skipped
    let verdicts: Vec<Verdict> = pr.checks.iter().map(|check| check.verdict).collect();

    assert_eq!(
        verdicts,
        vec![Verdict::Fail, Verdict::Pending, Verdict::Pass, Verdict::Skip]
    );
    assert_eq!(pr.checks[2].secs, 27);
    assert_eq!(pr.check_counts(), (1, 1, 1, 1));

    // a failed check (not required) on a blocked PR → checks failed
    assert_eq!(pr.ready(), Ready::ChecksFailed);
}

#[test]
fn readiness() {
    let mut pr = parse_detail(&PrRef::new("a/b", 7), &fixture()).unwrap();

    pr.checks.retain(|check| check.verdict == Verdict::Pass);

    assert_eq!(pr.ready(), Ready::NeedsApproval);

    pr.review = "APPROVED".into();
    pr.merge_state = "BEHIND".into();

    assert_eq!(pr.ready(), Ready::Behind);
    assert!(pr.needs_update() && !pr.conflicts());

    pr.merge_state = "CLEAN".into();

    assert_eq!(pr.ready(), Ready::Mergeable);

    pr.mergeable = "CONFLICTING".into();

    assert_eq!(pr.ready(), Ready::Conflicts);

    pr.state = State::Draft;

    assert_eq!(pr.ready(), Ready::Draft);
}

#[test]
fn list_items_parse() {
    let search = json!({"nodes": [{"number": 3, "title": "t", "state": "MERGED", "isDraft": false, "createdAt": "2026-01-01T00:00:00Z", "repository": {"nameWithOwner": "a/b"}}, {}]});
    let items = parse_items(&search);

    assert_eq!(items.len(), 1);
    assert_eq!(
        (items[0].pr.label(), items[0].state),
        ("b#3".to_string(), State::Merged)
    );
}

/// `CODEHEAD_GH_LIVE=1` cargo test live_ -- --ignored: the queries against github.com.
#[test]
#[ignore = "requires GitHub authentication and network access"]
fn live_queries() {
    let pr = fetch(&PrRef::new("cli/cli", 15100)).unwrap();

    assert!(
        !pr.title.is_empty() && !pr.checks.is_empty() && !pr.threads.is_empty(),
        "{pr:?}"
    );
    assert_ne!(mine(1).unwrap(), [] as [crate::infra::gh::Item; 0]);
    assert!(diff(&pr.pr, None).unwrap().starts_with("diff --git"));
    assert!(diff(&pr.pr, Some(&pr.commits[0].oid)).unwrap().contains("diff --git"));
    assert_ne!(labels("cli/cli").unwrap(), [] as [crate::infra::gh::Label; 0]);

    let _ = for_branch("cli/cli", &pr.head).unwrap();
}

#[test]
fn quotes() {
    assert_eq!(quote("x", "a\n\nb"), "@x wrote:\n> a\n>\n> b\n\n");
}
