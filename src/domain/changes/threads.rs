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
        let character_value = all
            .iter_mut()
            .find(|character| character.id == id)
            .ok_or_else(|| unknown(id))?;

        character_value.replies.push(Reply {
            by: by.to_string(),
            at: crate::engine::now_ms(),
            text: text.to_string(),
            unseen: !by.is_empty(),
        });

        if by.is_empty() {
            append_question_context(character_value);

            character_value.sent = delivered;
        }

        Ok(character_value.clone())
    })
}

fn append_question_context(character_value: &mut Comment) {
    if !character_value.open() {
        character_value.state = CState::Open;
        character_value.resolved_by.clear();
        character_value.note.clear();
    }
}

/// The text of comment ID of THREAD (REPLY None) or of its reply REPLY: TEXT. 0 text: the
/// comment / the reply goes.
pub(crate) fn edit(thread: &str, id: &str, reply: Option<usize>, text: &str) -> Result<(), String> {
    let text = text.trim();

    update_comments(thread, |all| {
        let i = all
            .iter()
            .position(|character| character.id == id)
            .ok_or_else(|| unknown(id))?;

        match reply {
            None if text.is_empty() => {
                all.remove(i);
            }
            None => all[i].text = text.to_string(),
            Some(result) if result >= all[i].replies.len() => return Err(format!("{id}: 0 reply {}", result + 1)),
            Some(result_value) if text.is_empty() => {
                all[i].replies.remove(result_value);
            }
            Some(result_2) => all[i].replies[result_2].text = text.to_string(),
        }

        Ok(())
    })
}

/// The owner saw the replies of comment ID of THREAD. True: one was unseen.
pub(crate) fn mark_seen(thread: &str, id: &str) -> bool {
    update_comments(thread, |all| {
        let mut changed = false;

        for result_value in all
            .iter_mut()
            .filter(|character| character.id == id)
            .flat_map(|character_value| character_value.replies.iter_mut())
            .filter(|result| result.unseen)
        {
            result_value.unseen = false;
            changed = true;
        }

        changed
    })
}

/// C's unseen agent replies.
pub(crate) fn unseen(comment: &Comment) -> usize {
    comment.replies.iter().filter(|result| result.unseen).count()
}

/// The last reply of C is the owner's and names an agent: it waits on that agent (a commit leaves
/// it open).
pub(crate) fn waits_on_agent(comment: &Comment) -> bool {
    comment
        .replies
        .last()
        .is_some_and(|result| result.by.is_empty() && names_agent(&result.text))
}

/// TEXT has an `@name` other than `@owner`.
fn names_agent(text: &str) -> bool {
    text.match_indices('@').any(|(i, _)| {
        let before = text[..i].chars().next_back();
        let word: String = text[i + 1..]
            .chars()
            .take_while(|character| character.is_ascii_alphanumeric() || *character == '-' || *character == '_')
            .collect();

        !before.is_some_and(|character_value| character_value.is_alphanumeric() || character_value == '`')
            && !word.is_empty()
            && !word.eq_ignore_ascii_case("owner")
    })
}

/// The agents of NAMES that TEXT names (`@name`), 0 of SKIP (the author).
pub(crate) fn mentioned(text: &str, names: &[String], skip: &str) -> Vec<String> {
    crate::engine::host::mentions(text, names)
        .into_iter()
        .filter(|count| !count.eq_ignore_ascii_case(skip))
        .collect()
}

/// Who wrote it, for an agent: `owner`, `REGI` (0 `@`: a prompt routes by `@name`).
fn who(by: &str) -> &str {
    if by.is_empty() { "owner" } else { by }
}

/// Comment C with its whole thread, for the agents a mention names: where it is, the lines it is
/// on, each text, the tools to answer.
pub(crate) fn context(comment: &Comment) -> String {
    let mut text_value = format!(
        "[codehead: comment {} on `{}:{}` (the owner's changes view), {}. Its thread:\n",
        comment.id,
        comment.path,
        comment.at(),
        if comment.open() { "open" } else { "resolved" }
    );

    for line in comment.quote.lines() {
        let _ = writeln!(text_value, "> {line}");
    }

    let mut add = |by: &str, text: &str| {
        let mut lines = text.lines();

        let _ = writeln!(text_value, "- {}: {}", who(by), lines.next().unwrap_or_default());

        for line_value in lines {
            let _ = writeln!(text_value, "  {line_value}");
        }
    };

    add(&comment.by, &comment.text);

    for result in &comment.replies {
        add(&result.by, &result.text);
    }

    let _ = write!(
        text_value,
        "Answer on the comment with the `comment_reply` tool (codehead MCP, id {}). Fixed or settled: `comment_status` resolved with a 1-line note.]",
        comment.id
    );

    text_value
}

/// The latest unseen agent reply of each comment of THREAD: (comment id, its time, its author,
/// its 1st line).
pub(crate) fn unseen_replies(thread: &str) -> Vec<(String, u64, String, String)> {
    super::read_comments(thread)
        .iter()
        .filter_map(|character| {
            let result_value = character.replies.iter().rev().find(|result| result.unseen)?;

            Some((
                character.id.clone(),
                result_value.at,
                result_value.by.clone(),
                format!(
                    "{} {}: {}",
                    character.id,
                    character.path,
                    result_value.text.lines().next().unwrap_or_default()
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

    if j.as_ref().is_some_and(|(thread_value, _)| thread_value == thread) {
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
