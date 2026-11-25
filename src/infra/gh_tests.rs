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
    let path_value = parse_detail(&PrRef::new("a/b", 7), &fixture()).unwrap();

    assert_eq!(
        (
            path_value.title.as_str(),
            path_value.body.as_str(),
            path_value.state,
            path_value.method.as_str()
        ),
        ("Add X", "body\nline", State::Open, "SQUASH")
    );
    assert_eq!(
        path_value.labels,
        vec![Label {
            name: "bug".into(),
            color: "d73a4a".into()
        }]
    );
    assert_eq!(path_value.requested, vec!["rev"]);

    // the review without a body: 0 thread; the line thread: its reply inside; latest first
    assert_eq!(path_value.threads.len(), 2);
    assert_eq!(path_value.threads[0].root.kind, Kind::Line);
    assert_eq!(path_value.threads[0].replies.len(), 1);
    assert!(path_value.threads[0].replies[0].mine);
    assert_eq!(path_value.threads[0].line, Some(4));
    assert_eq!(path_value.threads[0].path.as_deref(), Some("src/x.rs"));

    // failed first, pending, passed, skipped
    let value: Vec<Verdict> = path_value.checks.iter().map(|character| character.verdict).collect();

    assert_eq!(
        value,
        vec![Verdict::Fail, Verdict::Pending, Verdict::Pass, Verdict::Skip]
    );
    assert_eq!(path_value.checks[2].secs, 27);
    assert_eq!(path_value.check_counts(), (1, 1, 1, 1));

    // a failed check (not required) on a blocked PR → checks failed
    assert_eq!(path_value.ready(), Ready::ChecksFailed);
}

#[test]
fn readiness() {
    let mut path = parse_detail(&PrRef::new("a/b", 7), &fixture()).unwrap();

    path.checks.retain(|character| character.verdict == Verdict::Pass);

    assert_eq!(path.ready(), Ready::NeedsApproval);

    path.review = "APPROVED".into();
    path.merge_state = "BEHIND".into();

    assert_eq!(path.ready(), Ready::Behind);
    assert!(path.needs_update() && !path.conflicts());

    path.merge_state = "CLEAN".into();

    assert_eq!(path.ready(), Ready::Mergeable);

    path.mergeable = "CONFLICTING".into();

    assert_eq!(path.ready(), Ready::Conflicts);

    path.state = State::Draft;

    assert_eq!(path.ready(), Ready::Draft);
}

#[test]
fn list_items_parse() {
    let value = json!({"nodes": [{"number": 3, "title": "t", "state": "MERGED", "isDraft": false, "createdAt": "2026-01-01T00:00:00Z", "repository": {"nameWithOwner": "a/b"}}, {}]});
    let items = parse_items(&value);

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
    let path = fetch(&PrRef::new("cli/cli", 15100)).unwrap();

    assert!(
        !path.title.is_empty() && !path.checks.is_empty() && !path.threads.is_empty(),
        "{path:?}"
    );
    assert_ne!(mine(1).unwrap(), [] as [crate::infra::gh::Item; 0]);
    assert!(diff(&path.pr, None).unwrap().starts_with("diff --git"));
    assert!(
        diff(&path.pr, Some(&path.commits[0].oid))
            .unwrap()
            .contains("diff --git")
    );
    assert_ne!(labels("cli/cli").unwrap(), [] as [crate::infra::gh::Label; 0]);

    let _ = for_branch("cli/cli", &path.head).unwrap();
}

#[test]
fn quotes() {
    assert_eq!(quote("x", "a\n\nb"), "@x wrote:\n> a\n>\n> b\n\n");
}
