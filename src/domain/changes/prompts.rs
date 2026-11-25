use super::{Changes, Command, Path, Pr, Stdio, git};

/// The base branch for `gh pr create --base`: BASE without its remote (`origin/main` → `main`).
pub(crate) fn base_branch(base: &str) -> &str {
    base.split_once('/').map_or(base, |(_, name)| name)
}
/// The PR of URL as `repo#N` (`https://github.com/o/codehead/pull/12` → `codehead#12`).
pub(crate) fn pr_label(pr: &Pr) -> String {
    let repo = pr.url.split('/').nth(4).filter(|name| !name.is_empty());

    match repo {
        Some(repo) => format!("{repo}#{}", pr.number),
        None => format!("#{}", pr.number),
    }
}
/// The 1st step of each agent task: the checkout must be on BRANCH, else the agent stops (a
/// moved HEAD never gets pushed or a PR).
pub(crate) fn branch_guard(branch: &str) -> String {
    format!(
        "0. Check: `git rev-parse --abbrev-ref HEAD` prints `{branch}`. Any other output: stop, change nothing, report it.\n"
    )
}
/// The ship section's "create PR" (DRAFT: as a draft) for BRANCH: the task of its sub-thread.
pub(crate) fn pr_task(branch: &str, base: &str, draft: bool) -> String {
    let base_name = base_branch(base);

    let kind = if draft {
        "a draft pull request (`--draft`)"
    } else {
        "a pull request"
    };

    format!(
        "Open {kind} for the branch `{branch}` against `{base_name}`.\n\n\
         {guard}\
         1. Push the commits already made: `git push -u origin {branch}`. Never commit, stage, stash or discard: uncommitted changes stay as they are, out of the PR.\n\
         2. Read the branch's commits and diff against `{base}` (`git log {base}..{branch}`, `git diff {base}...{branch}`).\n\
         3. Title: imperative, max. 72 chars, in the repo's commit style. Body: what changed and why, how it was tested. The repo has a PR template (`.github/pull_request_template.md` or similar): fill it in.\n\
         4. `gh pr create --head {branch} --base {base_name}{d} --title \"…\" --body-file -` (the body on stdin).\n\n\
         End with the PR's URL as your last line.",
        guard = branch_guard(branch),
        d = if draft { " --draft" } else { "" }
    )
}
/// The ship section's "update PR description" of BRANCH's PR P: the task of its sub-thread.
pub(crate) fn pr_desc_task(branch: &str, pr: &Pr) -> String {
    let number = pr.number;

    format!(
        "Update the title and the description of pull request {label} ({url}) of the branch `{branch}` where they no longer match its changes.\n\n\
         1. Read the PR's title, body and full diff: `gh pr view {number} --json title,body,baseRefName,headRefName`, `gh pr diff {number}`. `headRefName` is not `{branch}`: stop, change nothing, report it.\n\
         2. Rewrite only what is wrong or missing: keep what still holds, keep the template's sections. Title: imperative, max. 72 chars.\n\
         3. `gh pr edit {number} --title \"…\" --body-file -` (only when a change is needed).\n\n\
         Change no code, make no commit. End with what changed (title / body), or \"title and body match the changes: 0 edits\".",
        label = pr_label(pr),
        url = pr.url
    )
}
/// The ship section's "update PR on the newest base" of BRANCH's PR P (BASE: `origin/main`): the
/// task of its sub-thread.
pub(crate) fn pr_rebase_task(branch: &str, base: &str, pr: &Pr) -> String {
    let base_name = base_branch(base);
    let remote = base.split_once('/').map_or("origin", |(remote, _)| remote);

    format!(
        "Update pull request {label} ({url}) of the branch `{branch}` on the newest `{base_name}`: rebase, resolve the conflicts, push.\n\n\
         {guard}\
         1. `git status --porcelain` prints anything: stop, change nothing, report it (the owner's uncommitted work stays untouched).\n\
         2. `git fetch {remote}`, then `git rebase {base}`.\n\
         3. A conflict: resolve each file, keep the intent of both sides; `git add` it, `git -c core.editor=true rebase --continue`. Repeat until the rebase is done. A conflict you cannot resolve with confidence: `git rebase --abort`, report it, push nothing.\n\
         4. Build and run the tests the repo has for the changed code; fix what the rebase broke, as new commits.\n\
         5. `git push --force-with-lease origin {branch}` (only this branch, never a plain `--force`).\n\n\
         End with the new head commit, what you decided per conflict, and the test result.",
        label = pr_label(pr),
        url = pr.url,
        guard = branch_guard(branch)
    )
}
/// The ship section's "ship" of BRANCH: the task of its sub-thread; the state of the branch now
/// along, so the agent skips the steps done.
pub(crate) fn ship_task(branch: &str, changes: &Changes, pr: Option<&Pr>) -> String {
    let base_name = base_branch(&changes.base);
    let unpushed = changes.commits.list.iter().filter(|commit| !commit.pushed).count();

    let (pr_now, checks) = match pr {
        Some(pr) => (
            format!(
                "open: {} ({}), checks ✓{} ✗{} ●{}",
                pr_label(pr),
                pr.url,
                pr.pass,
                pr.fail,
                pr.pending
            ),
            format!("gh pr checks {} --watch", pr.number),
        ),
        None => ("none".into(), format!("gh pr checks {branch} --watch")),
    };

    format!(
        "Ship the branch `{branch}`: do each step in order, skip each step already done.\n\n\
         State now: {files} uncommitted file(s), {unpushed} unpushed commit(s), {ahead} commit(s) ahead of `{base}`, pull request: {pr_now}.\n\n\
         {guard}\
         1. Commit: the uncommitted changes as commits in the repo's commit style (1 commit per topic). Leave out files that do not belong (secrets, local config, scratch).\n\
         2. Push: `git push -u origin {branch}`.\n\
         3. Pull request: open one if none exists (`gh pr create --head {branch} --base {base_name} --title \"…\" --body-file -`; fill in the repo's PR template).\n\
         4. Review: read the full PR diff as a reviewer, find correctness bugs. Fix the real ones, commit, push to `{branch}`. Then bring the PR's title + body up to date.\n\
         5. CI: `{checks}`. A check fails: read its log (`gh run view <id> --log-failed`), fix the cause, commit, push to `{branch}`, watch again. Repeat until every check passes.\n\n\
         Never merge the pull request. Never push to any branch but `{branch}`. End with the PR's URL and the checks' state.",
        guard = branch_guard(branch),
        files = changes.files.len(),
        ahead = changes.ahead,
        base = changes.base
    )
}
/// `src/a.rs` / `3 files`.
pub(crate) fn files_label(paths: &[String]) -> String {
    if paths.len() == 1 {
        paths[0].clone()
    } else {
        format!("{} files", paths.len())
    }
}
/// A commit message for DIR's staged diff by MODEL (blocking): subject + body, as the repo's
/// recent subjects look. The owner edits it before the commit.
pub(crate) fn draft_message(dir: &Path, model: &str) -> Result<String, String> {
    let diff = git(dir, &["diff", "--cached", "--no-color", "--no-ext-diff", "-U2"])?;

    if diff.trim().is_empty() {
        return Err("0 staged changes".into());
    }

    let log = git(dir, &["log", "-8", "--format=%s"]).unwrap_or_default();
    let diff: String = diff.lines().take(1500).collect::<Vec<_>>().join("\n");
    let prompt = format!(
        "Write the git commit message for this staged diff.\n- Subject: max. 72 chars, imperative, the same style (prefixes like feat: / fix:) as these recent subjects:\n{log}\n- Then 1 blank line and a short body: what changed and why, as `- ` lines, max. 72 chars each.\n- Output only the message.\n\n```diff\n{diff}\n```"
    );
    let out = Command::new("claude")
        .current_dir(dir)
        .args([
            "-p",
            "--model",
            model,
            "--tools",
            "",
            "--strict-mcp-config",
            "--setting-sources",
            "",
            "--disable-slash-commands",
            "--no-session-persistence",
        ])
        .args([
            "--system-prompt",
            "You write git commit messages. Output only the message: 0 notes, 0 fences, 0 extra text.",
        ])
        .args(["--output-format", "json"])
        .arg(prompt)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|error| format!("claude: {error}"))?;
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|error| format!("claude: {error}"))?;
    let text = value["result"].as_str().unwrap_or_default();

    if value["is_error"] == true {
        return Err(text.lines().next().unwrap_or("claude failed").to_string());
    }

    Ok(clean_message(text))
}
/// A model's commit message without fences and without a trailing note paragraph.
pub(crate) fn clean_message(text: &str) -> String {
    let body = text.trim().trim_start_matches("```").trim_end_matches("```").trim();
    let paras: Vec<&str> = body
        .split("\n\n")
        .take_while(|para| !para.trim_start().starts_with("Note:") && !para.contains("doinstruct.tools"))
        .collect();

    paras.join("\n\n").trim().to_string()
}
