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

    for character in items {
        let file = character["file"].as_str().unwrap_or_default().trim();
        let text = character["text"].as_str().unwrap_or_default().trim();
        let line = character["line"].as_u64().unwrap_or(0) as u32;

        if file.is_empty() || text.is_empty() {
            return Err("each comment needs file and text".into());
        }

        // an absolute path: relative to its checkout (the work dir, a linked worktree of an
        // agent); a relative one: relative to the work dir
        let path_value = Path::new(file);

        let at = if path_value.is_absolute() {
            root.filter(|result| path_value.starts_with(result))
                .map(Path::to_path_buf)
                .or_else(|| path_value.parent().and_then(checkout_of))
        } else {
            root.map(Path::to_path_buf)
        };

        let rel = at
            .as_deref()
            .and_then(|result_value| path_value.strip_prefix(result_value).ok())
            .unwrap_or(path_value)
            .to_string_lossy()
            .trim_start_matches("./")
            .to_string();
        let level = rank::Level::parse(character["severity"].as_str().unwrap_or_default()).unwrap_or_default();
        let root = at
            .map(|result_2| result_2.to_string_lossy().into_owned())
            .unwrap_or_default();

        add.push(Comment {
            path: rel,
            line,
            text: text.to_string(),
            lines: character["lines"].as_str().unwrap_or_default().to_string(),
            level,
            by: agent.to_string(),
            root,
            ..Default::default()
        });
    }

    let at = crate::engine::now_ms();

    for character_value in &mut add {
        character_value.at = at;
    }

    Ok(update_comments(thread, |all| {
        let from = all.len();

        all.extend(add);
        give_ids(all);
        all[from..].iter().map(|character_2| character_2.id.clone()).collect()
    }))
}
/// MCP `comments`: THREAD's comments in STATE (`open`, `resolved`, `all`) as JSON for an agent.
pub(crate) fn list_comments(thread: &str, state: &str) -> String {
    let rows: Vec<serde_json::Value> = read_comments(thread)
        .iter()
        .filter(|character| match state {
            "resolved" => !character.open(),
            "all" => true,
            _ => character.open(),
        })
        .map(|character_value| {
            serde_json::json!({
                "id": character_value.id, "file": character_value.path, "line": character_value.line, "lines": character_value.lines, "severity": character_value.level.label(),
                "author": if character_value.by.is_empty() { "owner" } else { character_value.by.as_str() },
                "state": if character_value.open() { "open" } else { "resolved" },
                "resolved_by": character_value.resolved_by, "note": character_value.note, "text": character_value.text,
                "replies": character_value.replies.iter().map(|reply| serde_json::json!({"author": if reply.by.is_empty() { "owner" } else { reply.by.as_str() }, "text": reply.text})).collect::<Vec<_>>(),
                "last_by": character_value.replies.last().map_or(if character_value.by.is_empty() { "owner" } else { character_value.by.as_str() }, |reply| if reply.by.is_empty() { "owner" } else { reply.by.as_str() }),
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
            .filter(|id| !all.iter().any(|character| &character.id == *id))
            .collect();

        if !unknown.is_empty() {
            return Err(format!(
                "unknown comment ids: {} (the `comments` tool lists them)",
                unknown.iter().map(|text| text.as_str()).collect::<Vec<_>>().join(", ")
            ));
        }

        for character_2 in all
            .iter_mut()
            .filter(|character_value| ids.contains(&character_value.id))
        {
            character_2.state = to;

            character_2.resolved_by = record_comment_resolution(agent, to);

            character_2.note = note.trim().to_string();

            // reopened: the next batch takes it again
            update_comment_note(to, character_2);
        }

        Ok(format!(
            "{} comment{} {state}",
            ids.len(),
            if ids.len() == 1 { "" } else { "s" }
        ))
    })
}

fn record_comment_resolution(agent: &str, to: CState) -> String {
    if to == CState::Resolved {
        agent.to_string()
    } else {
        String::new()
    }
}

fn update_comment_note(to: CState, character_2: &mut Comment) {
    if to == CState::Open {
        character_2.sent = false;
    }
}
/// The batch message of COMMENTS (0 comments: None): each with its id, severity, author.
pub(crate) fn batch(comments: &[&Comment]) -> Option<String> {
    if comments.is_empty() {
        return None;
    }

    let mut text_value = String::from(
        "Review comments on the changes (file:line, new side). Address each one. After a fix, or with a reason not to fix, call the `comment_status` tool (codehead MCP) with its id, state `resolved` and a 1-line note:\n",
    );

    if comments.iter().any(|character| !character.replies.is_empty()) {
        text_value.insert_str(
            text_value.len() - 2,
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
            text_value,
            "- {} [{}] `{}:{}` by {}: {}",
            comment.id,
            comment.level.label(),
            comment.path,
            comment.at(),
            who(&comment.by),
            lines.next().unwrap_or_default()
        );

        for line in lines {
            let _ = writeln!(text_value, "  {line}");
        }

        for result in &comment.replies {
            let mut lines = result.text.lines();

            let _ = writeln!(
                text_value,
                "  ↳ {}: {}",
                who(&result.by),
                lines.next().unwrap_or_default()
            );

            append_batch_prompts(&mut text_value, lines);
        }
    }

    Some(text_value.trim_end().to_string())
}

fn append_batch_prompts(text_value: &mut String, lines: std::str::Lines<'_>) {
    for line_value in lines {
        let _ = writeln!(text_value, "    {line_value}");
    }
}
