use super::{
    COMMENT, Command, Item, Kind, Label, Op, Pr, PrRef, Stdio, Value, Write, json, nodes, parse_detail, parse_items,
};

/// gh ARGS (STDIN piped in), its stdout. Err: its 1st error line.
pub(super) fn gh(args: &[&str], stdin: Option<&str>) -> Result<String, String> {
    if cfg!(test) && std::env::var_os("CODEHEAD_GH_LIVE").is_none() {
        return Err("gh: 0 calls in tests".into());
    }

    // the setup tutorial: 0 GitHub calls (fetch, diff, run, labels: its mock PR)
    if crate::infra::tutorial::active().is_some() {
        return Err("tutorial: 0 GitHub calls".into());
    }

    let mut child = Command::new("gh")
        .args(args)
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("gh: {error}"))?;

    if let (Some(text), Some(mut i)) = (stdin, child.stdin.take()) {
        let _ = i.write_all(text.as_bytes());
    }

    let output = child.wait_with_output().map_err(|error| format!("gh: {error}"))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&output.stderr);

        Err(err
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or("gh failed")
            .to_string())
    }
}

pub(super) fn graphql(query: &str, vars: &[(&str, Value)]) -> Result<Value, String> {
    let body = json!({"query": query, "variables": vars.iter().map(|(key, value)| (key.to_string(), value.clone())).collect::<serde_json::Map<_, _>>()});
    let out = gh(&["api", "graphql", "--input", "-"], Some(&body.to_string()))?;
    let value: Value = serde_json::from_str(&out).map_err(|error| format!("gh: {error}"))?;

    if let Some(first) = value["errors"].as_array().and_then(|errors| errors.first()) {
        return Err(first["message"].as_str().unwrap_or("GraphQL error").to_string());
    }

    Ok(value["data"].clone())
}

pub(super) fn detail_query() -> String {
    format!(
        "query($o:String!,$r:String!,$n:Int!){{viewer{{login}} repository(owner:$o,name:$r){{viewerDefaultMergeMethod autoMergeAllowed pullRequest(number:$n){{
id number title body url state isDraft createdAt mergedAt closedAt author{{login}} baseRefName headRefName headRefOid additions deletions changedFiles
mergeable mergeStateStatus reviewDecision viewerCanUpdate autoMergeRequest{{mergeMethod}}
isMergeQueueEnabled isInMergeQueue mergeQueueEntry{{position state estimatedTimeToMerge}}
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
pub(crate) fn fetch(pr: &PrRef) -> Result<Pr, String> {
    if let Some(repo) = crate::infra::tutorial::active() {
        return Ok(crate::infra::tutorial::mock_pr(pr, &repo));
    }

    let (owner, name) = pr.owner_name();
    let data = graphql(
        &detail_query(),
        &[("o", json!(owner)), ("r", json!(name)), ("n", json!(pr.number))],
    )?;

    parse_detail(pr, &data).ok_or_else(|| format!("{}: 0 such pull request", pr.label()))
}

/// The viewer's PRs (every repo, every state), newest first: max. PAGES x 100.
pub(crate) fn mine(pages: usize) -> Result<Vec<Item>, String> {
    let query = "query($q:String!,$after:String){search(query:$q,type:ISSUE,first:100,after:$after){pageInfo{hasNextPage endCursor} nodes{... on PullRequest{number title state isDraft createdAt headRefName repository{nameWithOwner}}}}}";
    let mut out = Vec::new();
    let mut after = Value::Null;

    for _ in 0..pages.max(1) {
        let data = graphql(
            query,
            &[
                ("q", json!("author:@me is:pr sort:created-desc")),
                ("after", after.clone()),
            ],
        )?;

        out.extend(parse_items(&data["search"]));
        let pi = &data["search"]["pageInfo"];

        if pi["hasNextPage"] != true {
            break;
        }

        after = pi["endCursor"].clone();
    }

    Ok(out)
}

/// The PRs of BRANCH (as head) in REPO, every state.
pub(crate) fn for_branch(repo: &str, branch: &str) -> Result<Vec<u64>, String> {
    let (owner, name) = repo.split_once('/').ok_or("0 owner")?;
    let query = "query($o:String!,$r:String!,$b:String!){repository(owner:$o,name:$r){pullRequests(headRefName:$b,first:10,orderBy:{field:CREATED_AT,direction:DESC}){nodes{number}}}}";
    let data = graphql(query, &[("o", json!(owner)), ("r", json!(name)), ("b", json!(branch))])?;

    Ok(nodes(&data["repository"]["pullRequests"])
        .filter_map(|node| node["number"].as_u64())
        .collect())
}

/// The labels of REPO.
pub(crate) fn labels(repo: &str) -> Result<Vec<Label>, String> {
    if crate::infra::tutorial::active().is_some() {
        return Ok(vec![
            Label {
                name: "docs".into(),
                color: "0075ca".into(),
            },
            Label {
                name: "crew".into(),
                color: "7057ff".into(),
            },
        ]);
    }

    let out = gh(
        &["label", "list", "-R", repo, "--limit", "500", "--json", "name,color"],
        None,
    )?;
    let value: Value = serde_json::from_str(&out).map_err(|error| error.to_string())?;

    Ok(value
        .as_array()
        .into_iter()
        .flatten()
        .map(|label| Label {
            name: label["name"].as_str().unwrap_or_default().into(),
            color: label["color"].as_str().unwrap_or_default().into(),
        })
        .collect())
}

/// The diff of R: COMMIT's own, else the whole PR against the merge base.
pub(crate) fn diff(pr: &PrRef, commit: Option<&str>) -> Result<String, String> {
    if let Some(repo) = crate::infra::tutorial::active() {
        return Ok(crate::infra::tutorial::mock_diff(&repo, commit));
    }

    match commit {
        Some(sha) => gh(
            &[
                "api",
                &format!("repos/{}/commits/{sha}", pr.repo),
                "-H",
                "Accept: application/vnd.github.diff",
            ],
            None,
        ),
        None => gh(&["pr", "diff", &pr.number.to_string(), "-R", &pr.repo], None),
    }
}

/// Run OP on R.
pub(crate) fn run(pr: &PrRef, op: &Op) -> Result<(), String> {
    if let Some(repo) = crate::infra::tutorial::active() {
        return crate::infra::tutorial::mock_run(&repo, op);
    }

    let number = pr.number.to_string();

    match op {
        Op::Title(title) => pr_command(pr, &["edit", &number, "--title", title], None),
        Op::Body(body) => pr_command(pr, &["edit", &number, "--body-file", "-"], Some(body)),
        Op::Close => pr_command(pr, &["close", &number], None),
        Op::Reopen => pr_command(pr, &["reopen", &number], None),
        Op::Ready => pr_command(pr, &["ready", &number], None),
        Op::Draft => pr_command(pr, &["ready", &number, "--undo"], None),
        Op::AddLabel(label) => pr_command(pr, &["edit", &number, "--add-label", label], None),
        Op::RemoveLabel(label) => pr_command(pr, &["edit", &number, "--remove-label", label], None),
        Op::Comment(comment) => pr_command(pr, &["comment", &number, "--body-file", "-"], Some(comment)),
        Op::Reply(id, body) => api_command(
            "POST",
            &(format!("repos/{}/pulls/{number}/comments/{id}/replies", pr.repo)),
            Some(json!({"body": body})),
        ),
        Op::Edit(Kind::Issue, id, body) => api_command(
            "PATCH",
            &(format!("repos/{}/issues/comments/{id}", pr.repo)),
            Some(json!({"body": body})),
        ),
        Op::Edit(Kind::Line, id, body) => api_command(
            "PATCH",
            &(format!("repos/{}/pulls/comments/{id}", pr.repo)),
            Some(json!({"body": body})),
        ),
        Op::Edit(Kind::Review, id, body) => api_command(
            "PUT",
            &(format!("repos/{}/pulls/{number}/reviews/{id}", pr.repo)),
            Some(json!({"body": body})),
        ),
        Op::Delete(Kind::Issue, id) => {
            api_command("DELETE", &(format!("repos/{}/issues/comments/{id}", pr.repo)), None)
        }
        Op::Delete(Kind::Line, id) => api_command("DELETE", &(format!("repos/{}/pulls/comments/{id}", pr.repo)), None),
        Op::Delete(Kind::Review, _) => Err("a review body cannot be deleted: edit it".into()),
        Op::Merge(method, auto) => merge_pr(pr, &number, method, *auto),
        Op::DisableAuto => pr_command(pr, &["merge", &number, "--disable-auto"], None),
        Op::Dequeue(id) => graphql(
            "mutation($id:ID!){dequeuePullRequest(input:{id:$id}){clientMutationId}}",
            &[("id", json!(id))],
        )
        .map(|_| ()),
    }
}

/// Quote BODY as a markdown reply to AUTHOR.
pub(crate) fn quote(author: &str, body: &str) -> String {
    let quoted: Vec<String> = body
        .lines()
        .map(|line| {
            if line.is_empty() {
                ">".to_string()
            } else {
                format!("> {line}")
            }
        })
        .collect();

    format!("@{author} wrote:\n{}\n\n", quoted.join("\n"))
}

fn pr_command(reference: &PrRef, args: &[&str], stdin: Option<&str>) -> Result<(), String> {
    let mut command = vec!["pr"];

    command.extend_from_slice(args);
    command.extend(["-R", reference.repo.as_str()]);
    gh(&command, stdin).map(|_| ())
}
fn api_command(method: &str, path: &str, body: Option<Value>) -> Result<(), String> {
    let mut command = vec!["api", "-X", method, path];
    let text = body.map(|value| value.to_string());

    if text.is_some() {
        command.extend(["--input", "-"]);
    }

    gh(&command, text.as_deref()).map(|_| ())
}

fn merge_pr(pr: &PrRef, number: &str, method: &str, auto: bool) -> Result<(), String> {
    let flag = match method {
        "SQUASH" => "--squash",
        "REBASE" => "--rebase",
        _ => "--merge",
    };

    let mut args = vec!["merge", number, flag];

    if auto {
        args.push("--auto");
    }

    pr_command(pr, &args, None)
}
