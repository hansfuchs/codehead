use super::{CState, Comment, Path, checkout_of, give_ids, rank, read_comments, update_comments};
use std::fmt::Write as _;

/// MCP `comment`: AGENT's comments (`[{file, line, lines?, severity, text}]`) into the comments
/// of THREAD (the changes view reads them at its next draw). ROOT: the worktree (absolute paths
/// in it: relative). Returns the new comments' ids.
pub(crate) fn agent_comments(
    thread: &str,
    agent: &str,
    root: Option<&Path>,
    list: &serde_json::Value,
) -> Result<Vec<String>, String> {
    let items = list.as_array().ok_or("comments: an array required")?;
    let mut add = Vec::new();

    for item in items {
        let file = item["file"].as_str().unwrap_or_default().trim();
        let text = item["text"].as_str().unwrap_or_default().trim();
        let line = item["line"].as_u64().unwrap_or(0) as u32;

        if file.is_empty() || text.is_empty() {
            return Err("each comment needs file and text".into());
        }

        // an absolute path: relative to its checkout (the work dir, a linked worktree of an
        // agent); a relative one: relative to the work dir
        let path = Path::new(file);

        let at = if path.is_absolute() {
            root.filter(|worktree| path.starts_with(worktree))
                .map(Path::to_path_buf)
                .or_else(|| path.parent().and_then(checkout_of))
        } else {
            root.map(Path::to_path_buf)
        };

        let rel = at
            .as_deref()
            .and_then(|checkout| path.strip_prefix(checkout).ok())
            .unwrap_or(path)
            .to_string_lossy()
            .trim_start_matches("./")
            .to_string();
        let level = rank::Level::parse(item["severity"].as_str().unwrap_or_default()).unwrap_or_default();
        let root = at
            .map(|checkout| checkout.to_string_lossy().into_owned())
            .unwrap_or_default();

        add.push(Comment {
            path: rel,
            line,
            text: text.to_string(),
            lines: item["lines"].as_str().unwrap_or_default().to_string(),
            level,
            by: agent.to_string(),
            root,
            ..Default::default()
        });
    }

    let at = crate::engine::now_ms();

    for comment in &mut add {
        comment.at = at;
    }

    Ok(update_comments(thread, |all| {
        let from = all.len();

        all.extend(add);
        give_ids(all);
        all[from..].iter().map(|comment| comment.id.clone()).collect()
    }))
}
/// MCP `comments`: THREAD's comments in STATE (`open`, `resolved`, `all`) as JSON for an agent.
pub(crate) fn list_comments(thread: &str, state: &str) -> String {
    let rows: Vec<serde_json::Value> = read_comments(thread)
        .iter()
        .filter(|comment| match state {
            "resolved" => !comment.open(),
            "all" => true,
            _ => comment.open(),
        })
        .map(|comment| {
            serde_json::json!({
                "id": comment.id, "file": comment.path, "line": comment.line, "lines": comment.lines, "severity": comment.level.label(),
                "author": if comment.by.is_empty() { "owner" } else { comment.by.as_str() },
                "state": if comment.open() { "open" } else { "resolved" },
                "resolved_by": comment.resolved_by, "note": comment.note, "text": comment.text,
                "replies": comment.replies.iter().map(|reply| serde_json::json!({"author": if reply.by.is_empty() { "owner" } else { reply.by.as_str() }, "text": reply.text})).collect::<Vec<_>>(),
                "last_by": comment.replies.last().map_or(if comment.by.is_empty() { "owner" } else { comment.by.as_str() }, |reply| if reply.by.is_empty() { "owner" } else { reply.by.as_str() }),
            })
        })
        .collect();

    serde_json::to_string_pretty(&rows).unwrap_or_default()
}
/// MCP `comment_status`: AGENT sets comments IDS of THREAD to STATE (`resolved` / `open`) with
/// NOTE (what fixed it, why not). Ok: what changed.
pub(crate) fn set_comment_state(
    thread: &str,
    agent: &str,
    ids: &[String],
    state: &str,
    note: &str,
) -> Result<String, String> {
    let to = match state {
        "resolved" => CState::Resolved,
        "open" => CState::Open,
        _ => return Err("state: resolved or open".into()),
    };

    update_comments(thread, |all| {
        let unknown: Vec<&String> = ids
            .iter()
            .filter(|id| !all.iter().any(|comment| &comment.id == *id))
            .collect();

        if !unknown.is_empty() {
            return Err(format!(
                "unknown comment ids: {} (the `comments` tool lists them)",
                unknown.iter().map(|id| id.as_str()).collect::<Vec<_>>().join(", ")
            ));
        }

        for comment in all.iter_mut().filter(|comment| ids.contains(&comment.id)) {
            comment.state = to;

            comment.resolved_by = resolver(agent, to);

            comment.note = note.trim().to_string();

            // reopened: the next batch takes it again
            requeue_reopened(to, comment);
        }

        Ok(format!(
            "{} comment{} {state}",
            ids.len(),
            if ids.len() == 1 { "" } else { "s" }
        ))
    })
}

fn resolver(agent: &str, to: CState) -> String {
    if to == CState::Resolved {
        agent.to_string()
    } else {
        String::new()
    }
}

fn requeue_reopened(to: CState, comment: &mut Comment) {
    if to == CState::Open {
        comment.sent = false;
    }
}
/// The batch message of COMMENTS (0 comments: None): each with its id, severity, author.
pub(crate) fn batch(comments: &[&Comment]) -> Option<String> {
    if comments.is_empty() {
        return None;
    }

    let mut message = String::from(
        "Review comments on the changes (file:line, new side). Address each one. After a fix, or with a reason not to fix, call the `comment_status` tool (codehead MCP) with its id, state `resolved` and a 1-line note:\n",
    );

    if comments.iter().any(|comment| !comment.replies.is_empty()) {
        message.insert_str(
            message.len() - 2,
            ". A question in a thread (↳ replies): answer it with the `comment_reply` tool",
        );
    }

    // authors without `@`: the thread view routes a message by its @names
    let who = |by: &str| {
        if by.is_empty() {
            "you".to_string()
        } else {
            by.to_string()
        }
    };

    for comment in comments {
        let mut lines = comment.text.lines();

        let _ = writeln!(
            message,
            "- {} [{}] `{}:{}` by {}: {}",
            comment.id,
            comment.level.label(),
            comment.path,
            comment.at(),
            who(&comment.by),
            lines.next().unwrap_or_default()
        );

        for line in lines {
            let _ = writeln!(message, "  {line}");
        }

        for reply in &comment.replies {
            let mut lines = reply.text.lines();

            let _ = writeln!(message, "  ↳ {}: {}", who(&reply.by), lines.next().unwrap_or_default());

            append_reply_lines(&mut message, lines);
        }
    }

    Some(message.trim_end().to_string())
}

fn append_reply_lines(message: &mut String, lines: std::str::Lines<'_>) {
    for line in lines {
        let _ = writeln!(message, "    {line}");
    }
}
