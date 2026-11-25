use super::{Check, Comment, Commit, Item, Kind, Label, Pr, PrRef, Queued, State, Thread, Value, Verdict, iso};

pub(super) fn comment(value: &Value, kind: Kind) -> Comment {
    Comment {
        kind,
        id: value["databaseId"].as_u64().unwrap_or(0),
        author: value["author"]["login"].as_str().unwrap_or("ghost").to_string(),
        body: value["body"].as_str().unwrap_or_default().replace("\r\n", "\n"),
        created: iso(value["createdAt"].as_str().unwrap_or_default()),
        url: value["url"].as_str().unwrap_or_default().to_string(),
        mine: value["viewerDidAuthor"].as_bool().unwrap_or(false),
        verdict: value["state"].as_str().unwrap_or_default().to_string(),
    }
}

pub(super) fn nodes(value: &Value) -> impl Iterator<Item = &Value> {
    value["nodes"].as_array().into_iter().flatten()
}

pub(super) fn parse_detail(reference: &PrRef, response: &Value) -> Option<Pr> {
    let repository = &response["repository"];
    let value = &repository["pullRequest"];

    value["number"].as_u64()?;
    let mut pull_request = parse_base(reference, repository, value, &response["viewer"]);

    pull_request.threads = parse_threads(value);
    pull_request.checks = parse_checks(value);
    pull_request.labels = nodes(&value["labels"]).map(parse_label).collect();
    pull_request.requested = nodes(&value["reviewRequests"]).filter_map(requested_reviewer).collect();
    pull_request.reviewers = nodes(&value["latestOpinionatedReviews"]).map(parse_reviewer).collect();
    pull_request.commits = nodes(&value["commits"]).map(parse_commit).collect();

    Some(pull_request)
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_string()
}
fn timestamp(value: &Value) -> Option<u64> {
    value.as_str().map(iso).filter(|&value| value > 0)
}

fn parse_base(reference: &PrRef, repository: &Value, value: &Value, viewer: &Value) -> Pr {
    Pr {
        pr: reference.clone(),
        title: text(&value["title"]),
        body: text(&value["body"]).replace("\r\n", "\n"),
        url: text(&value["url"]),
        state: State::of(
            value["state"].as_str().unwrap_or_default(),
            value["isDraft"].as_bool().unwrap_or(false),
        ),
        author: value["author"]["login"].as_str().unwrap_or("ghost").to_string(),
        created: timestamp(&value["createdAt"]).unwrap_or(0),
        merged: timestamp(&value["mergedAt"]),
        closed: timestamp(&value["closedAt"]),
        base: text(&value["baseRefName"]),
        head: text(&value["headRefName"]),
        head_oid: text(&value["headRefOid"]),
        additions: value["additions"].as_u64().unwrap_or(0),
        deletions: value["deletions"].as_u64().unwrap_or(0),
        files: value["changedFiles"].as_u64().unwrap_or(0),
        mergeable: text(&value["mergeable"]),
        merge_state: text(&value["mergeStateStatus"]),
        review: text(&value["reviewDecision"]),
        can_edit: value["viewerCanUpdate"].as_bool().unwrap_or(false),
        auto_merge: value["autoMergeRequest"]["mergeMethod"].as_str().map(String::from),
        id: text(&value["id"]),
        auto_allowed: repository["autoMergeAllowed"].as_bool().unwrap_or(false),
        queue: value["isMergeQueueEnabled"].as_bool().unwrap_or(false),
        queued: parse_queue(value),
        method: repository["viewerDefaultMergeMethod"]
            .as_str()
            .unwrap_or("MERGE")
            .to_string(),
        viewer: viewer["login"].as_str().unwrap_or_default().to_string(),
        ..Pr::default()
    }
}

fn parse_queue(value: &Value) -> Option<Queued> {
    if value["isInMergeQueue"] != true && !value["mergeQueueEntry"].is_object() {
        return None;
    }

    let entry = &value["mergeQueueEntry"];

    Some(Queued {
        position: entry["position"].as_u64().unwrap_or(0),
        state: text(&entry["state"]),
        eta: entry["estimatedTimeToMerge"].as_u64().unwrap_or(0),
    })
}

fn parse_threads(value: &Value) -> Vec<Thread> {
    let mut threads: Vec<Thread> = nodes(&value["comments"])
        .map(|value| Thread {
            root: comment(value, Kind::Issue),
            ..Thread::default()
        })
        .collect();

    threads.extend(
        nodes(&value["reviews"])
            .filter(|review| !review["body"].as_str().unwrap_or_default().trim().is_empty())
            .map(|value| Thread {
                root: comment(value, Kind::Review),
                ..Thread::default()
            }),
    );

    for review in nodes(&value["reviewThreads"]) {
        append_line_thread(&mut threads, review);
    }

    threads.sort_by_key(|thread| std::cmp::Reverse(thread.root.created));
    threads
}

fn append_line_thread(threads: &mut Vec<Thread>, value: &Value) {
    let mut comments = nodes(&value["comments"]).map(|value| comment(value, Kind::Line));

    let Some(root) = comments.next() else {
        return;
    };

    threads.push(Thread {
        root,
        replies: comments.collect(),
        path: value["path"].as_str().map(String::from),
        line: value["line"]
            .as_u64()
            .or_else(|| value["originalLine"].as_u64())
            .map(|line| line as u32),
        resolved: value["isResolved"].as_bool().unwrap_or(false),
        outdated: value["isOutdated"].as_bool().unwrap_or(false),
    });
}

fn parse_checks(value: &Value) -> Vec<Check> {
    let mut checks: Vec<Check> = nodes(&value["last"])
        .flat_map(|commit| {
            nodes(&commit["commit"]["statusCheckRollup"]["contexts"])
                .cloned()
                .collect::<Vec<_>>()
        })
        .map(|value| parse_check(&value))
        .collect();

    checks.sort_by(|first, second| {
        first
            .verdict
            .cmp(&second.verdict)
            .then_with(|| (&first.group, &first.name).cmp(&(&second.group, &second.name)))
    });

    checks
}

fn parse_check(value: &Value) -> Check {
    if value["__typename"] == "StatusContext" {
        return parse_status(value);
    }

    let verdict = match (
        value["status"].as_str().unwrap_or_default(),
        value["conclusion"].as_str().unwrap_or_default(),
    ) {
        ("COMPLETED", "SUCCESS" | "NEUTRAL") => Verdict::Pass,
        ("COMPLETED", "SKIPPED" | "STALE") => Verdict::Skip,
        ("COMPLETED", _) => Verdict::Fail,
        _ => Verdict::Pending,
    };

    let start = timestamp(&value["startedAt"]).unwrap_or(0);
    let completed = timestamp(&value["completedAt"]).unwrap_or(0);
    let workflow = value["checkSuite"]["workflowRun"]["workflow"]["name"]
        .as_str()
        .or_else(|| value["checkSuite"]["app"]["name"].as_str())
        .unwrap_or_default();

    Check {
        name: text(&value["name"]),
        group: workflow.to_string(),
        verdict,
        url: text(&value["detailsUrl"]),
        required: value["isRequired"].as_bool().unwrap_or(false),
        secs: completed.saturating_sub(start),
        description: text(&value["title"]),
    }
}

fn parse_status(value: &Value) -> Check {
    let verdict = match value["state"].as_str().unwrap_or_default() {
        "SUCCESS" => Verdict::Pass,
        "FAILURE" | "ERROR" => Verdict::Fail,
        _ => Verdict::Pending,
    };

    Check {
        name: text(&value["context"]),
        group: String::new(),
        verdict,
        url: text(&value["targetUrl"]),
        required: value["isRequired"].as_bool().unwrap_or(false),
        secs: 0,
        description: text(&value["description"]),
    }
}
fn parse_label(value: &Value) -> Label {
    Label {
        name: text(&value["name"]),
        color: text(&value["color"]),
    }
}
fn requested_reviewer(value: &Value) -> Option<String> {
    value["requestedReviewer"]["login"]
        .as_str()
        .or_else(|| value["requestedReviewer"]["name"].as_str())
        .map(String::from)
}
fn parse_reviewer(value: &Value) -> (String, String) {
    (
        value["author"]["login"].as_str().unwrap_or("ghost").to_string(),
        text(&value["state"]),
    )
}
fn parse_commit(value: &Value) -> Commit {
    let commit = &value["commit"];

    Commit {
        oid: text(&commit["oid"]),
        short: text(&commit["abbreviatedOid"]),
        headline: text(&commit["messageHeadline"]),
        date: timestamp(&commit["committedDate"]).unwrap_or(0),
    }
}

pub(super) fn parse_items(value: &Value) -> Vec<Item> {
    nodes(value)
        .filter_map(|node| {
            Some(Item {
                pr: PrRef::new(node["repository"]["nameWithOwner"].as_str()?, node["number"].as_u64()?),
                title: node["title"].as_str().unwrap_or_default().to_string(),
                state: State::of(
                    node["state"].as_str().unwrap_or_default(),
                    node["isDraft"].as_bool().unwrap_or(false),
                ),
                created: iso(node["createdAt"].as_str().unwrap_or_default()),
                head: node["headRefName"].as_str().unwrap_or_default().to_string(),
            })
        })
        .collect()
}
