//! Comment threads: replies on the review comments of the changes view, `@agent` mentions in
//! them (delivered at once), the owner's unseen agent replies (notifications).

use super::{CState, Comment, Reply, update_comments};

/// Comment ID of THREAD gets reply TEXT by BY ("": the owner). The owner's reply: a resolved
/// comment opens again (a resolved one is out of the file view); DELIVERED (a mention took the
/// thread to its agents): the next batch skips it, else takes it. An agent's reply: unseen for the
/// owner. Ok: the comment with the reply.
pub fn reply(thread: &str, by: &str, id: &str, text: &str, delivered: bool) -> Result<Comment, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("reply: text required".into());
    }
    update_comments(thread, |all| {
        let c = all.iter_mut().find(|c| c.id == id).ok_or_else(|| unknown(id))?;
        c.replies.push(Reply { by: by.to_string(), at: crate::engine::now_ms(), text: text.to_string(), unseen: !by.is_empty() });
        if by.is_empty() {
            if !c.open() {
                c.state = CState::Open;
                c.resolved_by.clear();
                c.note.clear();
            }
            c.sent = delivered;
        }
        Ok(c.clone())
    })
}

/// The text of comment ID of THREAD (REPLY None) or of its reply REPLY: TEXT. 0 text: the
/// comment / the reply goes.
pub fn edit(thread: &str, id: &str, reply: Option<usize>, text: &str) -> Result<(), String> {
    let text = text.trim();
    update_comments(thread, |all| {
        let i = all.iter().position(|c| c.id == id).ok_or_else(|| unknown(id))?;
        match reply {
            None if text.is_empty() => {
                all.remove(i);
            }
            None => all[i].text = text.to_string(),
            Some(r) if r >= all[i].replies.len() => return Err(format!("{id}: 0 reply {}", r + 1)),
            Some(r) if text.is_empty() => {
                all[i].replies.remove(r);
            }
            Some(r) => all[i].replies[r].text = text.to_string(),
        }
        Ok(())
    })
}

/// The owner saw the replies of comment ID of THREAD. True: one was unseen.
pub fn mark_seen(thread: &str, id: &str) -> bool {
    update_comments(thread, |all| {
        let mut changed = false;
        for r in all.iter_mut().filter(|c| c.id == id).flat_map(|c| c.replies.iter_mut()).filter(|r| r.unseen) {
            r.unseen = false;
            changed = true;
        }
        changed
    })
}

/// C's unseen agent replies.
pub fn unseen(c: &Comment) -> usize {
    c.replies.iter().filter(|r| r.unseen).count()
}

/// The last reply of C is the owner's and names an agent: it waits on that agent (a commit leaves
/// it open).
pub fn waits_on_agent(c: &Comment) -> bool {
    c.replies.last().is_some_and(|r| r.by.is_empty() && names_agent(&r.text))
}

/// TEXT has an `@name` other than `@owner`.
fn names_agent(text: &str) -> bool {
    text.match_indices('@').any(|(i, _)| {
        let before = text[..i].chars().next_back();
        let word: String = text[i + 1..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
        !before.is_some_and(|c| c.is_alphanumeric() || c == '`') && !word.is_empty() && !word.eq_ignore_ascii_case("owner")
    })
}

/// The agents of NAMES that TEXT names (`@name`), 0 of SKIP (the author).
pub fn mentioned(text: &str, names: &[String], skip: &str) -> Vec<String> {
    crate::engine::host::mentions(text, names).into_iter().filter(|n| !n.eq_ignore_ascii_case(skip)).collect()
}

/// Who wrote it, for an agent: `owner`, `REGI` (0 `@`: a prompt routes by `@name`).
fn who(by: &str) -> &str {
    if by.is_empty() { "owner" } else { by }
}

/// Comment C with its whole thread, for the agents a mention names: where it is, the lines it is
/// on, each text, the tools to answer.
pub fn context(c: &Comment) -> String {
    let mut s = format!("[codehead: comment {} on `{}:{}` (the owner's changes view), {}. Its thread:\n", c.id, c.path, c.at(), if c.open() { "open" } else { "resolved" });
    for l in c.quote.lines() {
        s.push_str(&format!("> {l}\n"));
    }

    let mut add = |by: &str, text: &str| {
        let mut lines = text.lines();
        s.push_str(&format!("- {}: {}\n", who(by), lines.next().unwrap_or_default()));
        for l in lines {
            s.push_str(&format!("  {l}\n"));
        }
    };
    add(&c.by, &c.text);
    for r in &c.replies {
        add(&r.by, &r.text);
    }
    s.push_str(&format!("Answer on the comment with the `comment_reply` tool (codehead MCP, id {}). Fixed or settled: `comment_status` resolved with a 1-line note.]", c.id));
    s
}

/// The latest unseen agent reply of each comment of THREAD: (comment id, its time, its author,
/// its 1st line).
pub fn unseen_replies(thread: &str) -> Vec<(String, u64, String, String)> {
    super::read_comments(thread)
        .iter()
        .filter_map(|c| {
            let r = c.replies.iter().rev().find(|r| r.unseen)?;
            Some((c.id.clone(), r.at, r.by.clone(), format!("{} {}: {}", c.id, c.path, r.text.lines().next().unwrap_or_default())))
        })
        .collect()
}

/// A reply note's jump: (thread, comment id) for the next view of that thread.
static JUMP: std::sync::Mutex<Option<(String, String)>> = std::sync::Mutex::new(None);

/// The next view of THREAD opens the changes view on comment ID.
pub fn jump_to(thread: &str, id: &str) {
    if let Ok(mut j) = JUMP.lock() {
        *j = Some((thread.to_string(), id.to_string()));
    }
}

/// The comment jump of THREAD, once.
pub fn take_jump(thread: &str) -> Option<String> {
    let mut j = JUMP.lock().ok()?;
    if j.as_ref().is_some_and(|(t, _)| t == thread) { j.take().map(|(_, id)| id) } else { None }
}

fn unknown(id: &str) -> String {
    format!("unknown comment id: {id} (the `comments` tool lists them)")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thread(name: &str) -> String {
        let id = format!("threads-{name}-{}", std::process::id());
        let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir(&id));
        super::super::agent_comments(&id, "REV", None, &serde_json::json!([{"file": "a.rs", "line": 3, "severity": "high", "text": "off by 1"}])).unwrap();
        id
    }

    #[test]
    fn replies_reopen_on_the_owner_and_stay_unseen_from_an_agent() {
        let id = thread("reply");
        super::super::set_comment_state(&id, "REV", &["c1".into()], "resolved", "fixed").unwrap();

        // the owner's reply: open again, in the next batch
        let c = reply(&id, "", "c1", "why 1? @REV", false).unwrap();
        assert!(c.open() && !c.sent && c.resolved_by.is_empty() && c.note.is_empty());
        assert!(waits_on_agent(&c));

        // the agent's reply: unseen until the owner looks
        let c = reply(&id, "REV", "c1", "the loop starts at 0", false).unwrap();
        assert_eq!((c.replies.len(), unseen(&c), waits_on_agent(&c)), (2, 1, false));
        assert_eq!(unseen_replies(&id), vec![("c1".to_string(), c.replies[1].at, "REV".to_string(), "c1 a.rs: the loop starts at 0".to_string())]);
        assert!(mark_seen(&id, "c1") && !mark_seen(&id, "c1"));
        assert!(unseen_replies(&id).is_empty());

        assert!(reply(&id, "REV", "c9", "x", false).unwrap_err().contains("c9"));
        assert!(reply(&id, "REV", "c1", "  ", false).is_err());
        let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir(&id));
    }

    #[test]
    fn edits_change_or_drop_a_reply_or_the_comment() {
        let id = thread("edit");
        reply(&id, "", "c1", "a", false).unwrap();
        reply(&id, "REV", "c1", "b", false).unwrap();

        edit(&id, "c1", Some(1), "b2").unwrap();
        edit(&id, "c1", Some(0), "").unwrap();
        edit(&id, "c1", None, "off by 2").unwrap();
        let c = &super::super::read_comments(&id)[0];
        assert_eq!((c.text.as_str(), c.replies.len(), c.replies[0].text.as_str()), ("off by 2", 1, "b2"));

        assert!(edit(&id, "c1", Some(5), "x").is_err());
        edit(&id, "c1", None, "").unwrap();
        assert!(super::super::read_comments(&id).is_empty());
        let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir(&id));
    }

    #[test]
    fn the_context_names_authors_without_at_and_the_tools() {
        let id = thread("ctx");
        reply(&id, "", "c1", "sure? @REV", false).unwrap();
        let c = &super::super::read_comments(&id)[0];

        let s = context(c);

        assert!(s.starts_with("[codehead: comment c1 on `a.rs:3`"), "{s}");
        assert!(s.contains("- REV: off by 1\n- owner: sure? @REV\n") && s.contains("`comment_reply`") && s.ends_with(']'), "{s}");
        assert_eq!(mentioned("ask @REV and @LEAD, not @owner", &["REV".into(), "LEAD".into()], "rev"), vec!["LEAD".to_string()]);
        assert!(!names_agent("cc @owner, mail a@b") && names_agent("@LEAD look"));
        let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir(&id));
    }
}
