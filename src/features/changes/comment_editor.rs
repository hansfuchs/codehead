use super::{
    Act, Comment, DLine, Kind, Modal, QLine, Spot, Target, View, comment_area, comments_stamp, lkey, read_comments,
    reply_area, thread_lines, threads,
};

impl View {
    /// `add_comment` with severity LEVEL.
    pub(super) fn add_comment_at(&mut self, spots: &[Spot], text: &str, level: crate::domain::changes::rank::Level) {
        add_comment_at_view(self, spots, text, level);
    }

    /// The diff lines of SPOT (`+ let b = 3;`): the selected lines of its file, else its line.
    pub(super) fn quote(&self, spot: &Spot) -> String {
        quote_view(self, spot)
    }

    /// The comment modal on SPOTS (the selection, else the cursor's line).
    pub(super) fn open_comment(&mut self, spots: Vec<Spot>) {
        open_comment_view(self, spots);
    }

    /// The comment modal on comment CI (REPLY: on its reply REPLY): its text to edit (0 text:
    /// it goes).
    pub(crate) fn edit_comment(&mut self, ci: usize, reply: Option<usize>) {
        edit_comment_view(self, ci, reply);
    }

    /// The reply modal on comment CI: its lines and thread above, the reply's text below.
    pub(crate) fn reply_comment(&mut self, ci: usize) {
        reply_comment_view(self, ci);
    }

    /// The view's pending changes to the file, then WRITE (a write of `threads` under the lock, on
    /// the thread's id), then the file read again.
    pub(super) fn write_through<T>(&mut self, write: impl FnOnce(&str) -> T) -> T {
        if self.comment_state.comments != self.comment_state.comments_base {
            self.save_comments();
        }

        let id = self.id.clone();
        let out = write(&id);

        self.comment_state.comments = read_comments(&id);
        self.comment_state.comments_base = self.comment_state.comments.clone();
        self.comment_state.comments_at = comments_stamp(&id);
        self.layout.built = None;

        out
    }

    /// The modal's save: a new comment, a reply, an edit (0 text: deleted). An `@agent` in the
    /// text: that agent gets the comment at once.
    pub(super) fn save_comment(&mut self) -> Act {
        save_comment_view(self)
    }
}

fn save_new_comment(view: &mut View, spots: &[Spot], level: super::rank::Level, message: &str) -> Act {
    let count = spots.len();

    view.add_comment_at(spots, message, level);
    let on = spots.iter().map(Spot::label).collect::<Vec<_>>().join(", ");

    // an @agent: the new comments go to it now (the batch skips them)
    let ids: Vec<String> = view
        .comment_state
        .comments
        .iter()
        .rev()
        .take(count)
        .rev()
        .map(|comment| comment.id.clone())
        .collect();
    let mut acts = ids.iter().filter_map(|id| mention(id, message, "", view));

    let Some(Act::Mention {
        to,
        mut text,
        mut context,
    }) = acts.next()
    else {
        return Act::Toast(format!(
            "comment on {on} · {} in the batch · ctrl+enter: send",
            view.comment_state.comments.len()
        ));
    };

    for more in acts {
        if let Act::Mention {
            text: more_text,
            context: more_context,
            ..
        } = more
        {
            text = format!("{text}\n{more_text}");
            context = format!("{context}\n\n{more_context}");
        }
    }

    for comment in view
        .comment_state
        .comments
        .iter_mut()
        .filter(|comment| ids.contains(&comment.id))
    {
        comment.sent = true;
    }

    view.save_comments();

    Act::Mention { to, text, context }
}

fn save_reply(view: &mut View, message: &str, id: &str) -> Act {
    let named = !threads::mentioned(message, &agent_names(view), "").is_empty();

    if let Err(error) = view.write_through(|thread_id| threads::reply(thread_id, "", id, message, named)) {
        return Act::Toast(error);
    }

    view.unfold_comment(id);
    view.rebuild_here();
    mention(id, message, "", view)
        .unwrap_or_else(|| Act::Toast(format!("reply on {id} · ctrl+enter: in the batch · @agent: at once")))
}

fn save_edited_reply(view: &mut View, message: &str, id: &str, reply: usize) -> Act {
    let old = view
        .comment_state
        .comments
        .iter()
        .find(|comment| comment.id == id)
        .and_then(|comment| comment.replies.get(reply))
        .map(|edited| edited.text.clone())
        .unwrap_or_default();

    if let Err(error) = view.write_through(|thread_id| threads::edit(thread_id, id, Some(reply), message)) {
        return Act::Toast(error);
    }

    view.rebuild_here();

    let done = if message.is_empty() {
        format!("{id}: reply deleted")
    } else {
        format!("{id}: reply saved")
    };

    mention(id, message, &old, view).unwrap_or(Act::Toast(done))
}

fn save_edited_comment(view: &mut View, level: super::rank::Level, message: &str, id: &str) -> Act {
    let Some(ci) = view.comment_state.comments.iter().position(|comment| comment.id == id) else {
        return Act::Toast(format!("{id}: gone"));
    };

    let old = view.comment_state.comments[ci].text.clone();

    if message.is_empty() {
        view.comment_state.comments.remove(ci);
    } else {
        view.comment_state.comments[ci].text = message.to_string();
        view.comment_state.comments[ci].level = level;
    }

    view.save_comments();
    view.rebuild_here();

    mention(id, message, &old, view).unwrap_or_else(|| {
        Act::Toast(format!(
            "comment saved · {} in the batch",
            view.comment_state.comments.len()
        ))
    })
}

/// The agents a text can name: the roster, the thread's members.
fn agent_names(view: &View) -> Vec<String> {
    let mut names: Vec<String> = view.agents.iter().map(|agent| agent.name.clone()).collect();

    names.extend(
        view.members
            .iter()
            .filter(|member| !view.agents.iter().any(|agent| &agent.name == *member))
            .cloned(),
    );
    names
}

/// The owner's TEXT on comment ID names agents (0 of OLD, the text before an edit): they get
/// it with the comment's thread at once.
fn mention(id: &str, text: &str, old: &str, view: &View) -> Option<Act> {
    let names = agent_names(view);
    let before = threads::mentioned(old, &names, "");
    let to: Vec<String> = threads::mentioned(text, &names, "")
        .into_iter()
        .filter(|name| !before.contains(name))
        .collect();

    if to.is_empty() {
        return None;
    }

    let comment = view.comment_state.comments.iter().find(|comment| comment.id == id)?;

    Some(Act::Mention {
        to,
        text: format!("on comment {} `{}:{}`: {text}", comment.id, comment.path, comment.at()),
        context: threads::context(comment),
    })
}

fn add_comment_at_view(view: &mut View, spots: &[Spot], text: &str, level: crate::domain::changes::rank::Level) {
    let text = text.trim();

    if text.is_empty() {
        return;
    }

    for spot in spots {
        let quote = view.quote(spot);
        let root = view.dir.to_string_lossy().into_owned();

        view.comment_state.comments.push(Comment {
            path: spot.path.clone(),
            line: spot.line,
            text: text.to_string(),
            lines: spot.lines.clone(),
            quote,
            level,
            root,
            ..Default::default()
        });
    }

    view.save_comments();
    view.selection_state.sel.clear();
    view.rebuild_here();
}

fn quote_view(view: &View, spot: &Spot) -> String {
    let Some(file) = view.changes.files.iter().find(|file| file.path == spot.path) else {
        return String::new();
    };

    let sign = |kind: Kind| match kind {
        Kind::Add => '+',
        Kind::Del => '-',
        Kind::Ctx => ' ',
    };
    let all = file.hunks.iter().flat_map(|hunk| &hunk.lines);

    let lines: Vec<&DLine> = match view.selection_state.sel.get(&spot.path) {
        Some(set) => all
            .filter(|line| lkey(line).is_some_and(|key| set.contains(&key)))
            .take(40)
            .collect(),
        None => all
            .clone()
            .find(|line| line.new == Some(spot.line))
            .or_else(|| {
                all.clone()
                    .find(|line| line.kind == Kind::Del && line.old == Some(spot.line))
            })
            .into_iter()
            .collect(),
    };

    lines
        .iter()
        .map(|line| format!("{} {}", sign(line.kind), line.text))
        .collect::<Vec<_>>()
        .join("\n")
}

fn open_comment_view(view: &mut View, spots: Vec<Spot>) {
    let mut quote = Vec::new();

    for (i, spot) in spots.iter().enumerate() {
        if spots.len() > 1 && i > 0 {
            quote.push(QLine::Gap);
        }

        if spots.len() > 1 {
            quote.push(QLine::Path(spot.label()));
        }

        quote.extend(view.quote(spot).lines().map(|line| QLine::Code(line.to_string())));
    }

    let title = spots.iter().map(Spot::label).collect::<Vec<_>>().join(", ");

    view.modal_state.modal = Some(Modal::Comment {
        title,
        quote,
        scroll: 0,
        target: Target::New,
        spots,
        text: comment_area(""),
        level: crate::domain::changes::rank::Level::default(),
        on_level: true,
    });
}

fn edit_comment_view(view: &mut View, ci: usize, reply: Option<usize>) {
    let Some(comment) = view.comment_state.comments.get(ci) else {
        return;
    };

    let title = format!("{}:{}", comment.path, comment.at());
    let mut quote: Vec<QLine> = comment
        .quote
        .lines()
        .map(|line| QLine::Code(line.to_string()))
        .collect();

    let (target, text) = match reply.and_then(|index| comment.replies.get(index).map(|edited| (index, edited))) {
        Some((index, edited)) => {
            quote.extend(thread_lines(comment, Some(index)));
            (Target::EditReply(comment.id.clone(), index), edited.text.clone())
        }
        None => (Target::Edit(comment.id.clone()), comment.text.clone()),
    };

    view.modal_state.modal = Some(Modal::Comment {
        title,
        quote,
        scroll: 0,
        target,
        spots: Vec::new(),
        text: comment_area(&text),
        level: comment.level,
        on_level: false,
    });
}

fn reply_comment_view(view: &mut View, ci: usize) {
    let Some(comment) = view.comment_state.comments.get(ci) else {
        return;
    };

    let title = format!("{}:{}", comment.path, comment.at());
    let mut quote: Vec<QLine> = comment
        .quote
        .lines()
        .map(|line| QLine::Code(line.to_string()))
        .collect();

    quote.extend(thread_lines(comment, None));

    // the newest replies in view (the draw caps the scroll)
    let scroll = quote.len();

    view.modal_state.modal = Some(Modal::Comment {
        title,
        quote,
        scroll,
        target: Target::Reply(comment.id.clone()),
        spots: Vec::new(),
        text: reply_area(),
        level: comment.level,
        on_level: false,
    });
}

fn save_comment_view(view: &mut View) -> Act {
    let Some(Modal::Comment {
        spots,
        target,
        text,
        level,
        ..
    }) = view.modal_state.modal.take()
    else {
        return Act::None;
    };

    let message = text.text().trim().to_string();

    match target {
        Target::Edit(id) => save_edited_comment(view, level, &message, &id),
        Target::EditReply(id, reply) => save_edited_reply(view, &message, &id, reply),
        Target::Reply(_) if message.is_empty() => Act::Toast("0 text: 0 reply".into()),
        Target::Reply(id) => save_reply(view, &message, &id),
        Target::New if message.is_empty() => Act::Toast("0 text: 0 comment".into()),
        Target::New => save_new_comment(view, &spots, level, &message),
    }
}
