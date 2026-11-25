//! Comment threads: replies on the review comments of the changes view, `@agent` mentions in
//! them (delivered at once), the owner's unseen agent replies (notifications).

use super::{CState, Comment, Reply, update_comments};
use std::fmt::Write as _;

/// Comment ID of THREAD gets reply TEXT by BY ("": the owner). The owner's reply: a resolved
/// comment opens again (a resolved one is out of the file view); DELIVERED (a mention took the
/// thread to its agents): the next batch skips it, else takes it. An agent's reply: unseen for the
/// owner. Ok: the comment with the reply.
pub(crate) fn reply(thread: &str, by: &str, id: &str, text: &str, delivered: bool) -> Result<Comment, String> {
    let text = text.trim();

    if text.is_empty() {
        return Err("reply: text required".into());
    }

    update_comments(thread, |all| {
        let comment = all
            .iter_mut()
            .find(|comment| comment.id == id)
            .ok_or_else(|| unknown(id))?;

        comment.replies.push(Reply {
            by: by.to_string(),
            at: crate::engine::now_ms(),
            text: text.to_string(),
            unseen: !by.is_empty(),
        });

        if by.is_empty() {
            reopen(comment);

            comment.sent = delivered;
        }

        Ok(comment.clone())
    })
}

fn reopen(comment: &mut Comment) {
    if !comment.open() {
        comment.state = CState::Open;
        comment.resolved_by.clear();
        comment.note.clear();
    }
}

/// The text of comment ID of THREAD (REPLY None) or of its reply REPLY: TEXT. 0 text: the
/// comment / the reply goes.
pub(crate) fn edit(thread: &str, id: &str, reply: Option<usize>, text: &str) -> Result<(), String> {
    let text = text.trim();

    update_comments(thread, |all| {
        let i = all
            .iter()
            .position(|comment| comment.id == id)
            .ok_or_else(|| unknown(id))?;

        match reply {
            None if text.is_empty() => {
                all.remove(i);
            }
            None => all[i].text = text.to_string(),
            Some(index) if index >= all[i].replies.len() => return Err(format!("{id}: 0 reply {}", index + 1)),
            Some(index) if text.is_empty() => {
                all[i].replies.remove(index);
            }
            Some(index) => all[i].replies[index].text = text.to_string(),
        }

        Ok(())
    })
}

/// The owner saw the replies of comment ID of THREAD. True: one was unseen.
pub(crate) fn mark_seen(thread: &str, id: &str) -> bool {
    update_comments(thread, |all| {
        let mut changed = false;

        for reply in all
            .iter_mut()
            .filter(|comment| comment.id == id)
            .flat_map(|comment| comment.replies.iter_mut())
            .filter(|reply| reply.unseen)
        {
            reply.unseen = false;
            changed = true;
        }

        changed
    })
}

/// C's unseen agent replies.
pub(crate) fn unseen(comment: &Comment) -> usize {
    comment.replies.iter().filter(|reply| reply.unseen).count()
}

/// The last reply of C is the owner's and names an agent: it waits on that agent (a commit leaves
/// it open).
pub(crate) fn waits_on_agent(comment: &Comment) -> bool {
    comment
        .replies
        .last()
        .is_some_and(|reply| reply.by.is_empty() && names_agent(&reply.text))
}

/// TEXT has an `@name` other than `@owner`.
fn names_agent(text: &str) -> bool {
    text.match_indices('@').any(|(i, _)| {
        let before = text[..i].chars().next_back();
        let word: String = text[i + 1..]
            .chars()
            .take_while(|character| character.is_ascii_alphanumeric() || *character == '-' || *character == '_')
            .collect();

        !before.is_some_and(|previous| previous.is_alphanumeric() || previous == '`')
            && !word.is_empty()
            && !word.eq_ignore_ascii_case("owner")
    })
}

/// The agents of NAMES that TEXT names (`@name`), 0 of SKIP (the author).
pub(crate) fn mentioned(text: &str, names: &[String], skip: &str) -> Vec<String> {
    crate::engine::host::mentions(text, names)
        .into_iter()
        .filter(|name| !name.eq_ignore_ascii_case(skip))
        .collect()
}

/// Who wrote it, for an agent: `owner`, `REGI` (0 `@`: a prompt routes by `@name`).
fn who(by: &str) -> &str {
    if by.is_empty() { "owner" } else { by }
}

/// Comment C with its whole thread, for the agents a mention names: where it is, the lines it is
/// on, each text, the tools to answer.
pub(crate) fn context(comment: &Comment) -> String {
    let mut message = format!(
        "[codehead: comment {} on `{}:{}` (the owner's changes view), {}. Its thread:\n",
        comment.id,
        comment.path,
        comment.at(),
        if comment.open() { "open" } else { "resolved" }
    );

    for line in comment.quote.lines() {
        let _ = writeln!(message, "> {line}");
    }

    let mut add = |by: &str, text: &str| {
        let mut lines = text.lines();

        let _ = writeln!(message, "- {}: {}", who(by), lines.next().unwrap_or_default());

        for line in lines {
            let _ = writeln!(message, "  {line}");
        }
    };

    add(&comment.by, &comment.text);

    for reply in &comment.replies {
        add(&reply.by, &reply.text);
    }

    let _ = write!(
        message,
        "Answer on the comment with the `comment_reply` tool (codehead MCP, id {}). Fixed or settled: `comment_status` resolved with a 1-line note.]",
        comment.id
    );

    message
}

/// The latest unseen agent reply of each comment of THREAD: (comment id, its time, its author,
/// its 1st line).
pub(crate) fn unseen_replies(thread: &str) -> Vec<(String, u64, String, String)> {
    super::read_comments(thread)
        .iter()
        .filter_map(|comment| {
            let reply = comment.replies.iter().rev().find(|reply| reply.unseen)?;

            Some((
                comment.id.clone(),
                reply.at,
                reply.by.clone(),
                format!(
                    "{} {}: {}",
                    comment.id,
                    comment.path,
                    reply.text.lines().next().unwrap_or_default()
                ),
            ))
        })
        .collect()
}

/// A reply note's jump: (thread, comment id) for the next view of that thread.
static JUMP: std::sync::Mutex<Option<(String, String)>> = std::sync::Mutex::new(None);

/// The next view of THREAD opens the changes view on comment ID.
pub(crate) fn jump_to(thread: &str, id: &str) {
    if let Ok(mut j) = JUMP.lock() {
        *j = Some((thread.to_string(), id.to_string()));
    }
}

/// The comment jump of THREAD, once.
pub(crate) fn take_jump(thread: &str) -> Option<String> {
    let mut j = JUMP.lock().ok()?;

    if j.as_ref().is_some_and(|(jumped, _)| jumped == thread) {
        j.take().map(|(_, id)| id)
    } else {
        None
    }
}

fn unknown(id: &str) -> String {
    format!("unknown comment id: {id} (the `comments` tool lists them)")
}

#[cfg(test)]
#[path = "threads_tests.rs"]
mod tests;
