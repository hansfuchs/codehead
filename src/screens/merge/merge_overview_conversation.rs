use super::{
    Edit, Kind, Line, Modifier, OverviewRows, Row, Span, Stop, Style, UnicodeWidthStr, gh, highlight, input_rows, theme,
};

pub(super) fn append_comments(overview: &mut OverviewRows<'_>) {
    // comments: latest first, replies in the box
    overview.rows.push(Row::new(Line::styled(
        format!("COMMENTS · {}", overview.pr.threads.len()),
        theme::bold(),
    )));

    append_new_comment(overview);

    for (index, thread) in overview.pr.threads.iter().enumerate() {
        append_thread(overview, index, thread);
    }

    overview.rows.push(Row::new(Line::raw("")));
}

fn comment_hint(comment: &gh::Comment) -> &'static str {
    if comment.mine {
        "  enter: edit / delete"
    } else if comment.kind == Kind::Line {
        "  enter: reply"
    } else {
        "  enter: quote in a reply"
    }
}

fn append_edit_input(overview: &mut OverviewRows<'_>) {
    if let Some((_, textarea)) = overview.edit {
        overview.rows.push(Row {
            line: Line::raw(""),
            stops: Vec::new(),
            input: input_rows(overview.width, textarea, false),
        });
    }
}

pub(super) fn append_description(overview: &mut OverviewRows<'_>) {
    // the description
    overview.rows.push(Row::stop(
        Line::styled(
            if overview.focus == Some(Stop::Body) {
                "DESCRIPTION  enter: edit"
            } else {
                "DESCRIPTION"
            },
            highlight(overview.focus, Stop::Body, theme::bold()),
        ),
        Stop::Body,
    ));

    match overview.edit {
        Some((Edit::Body, textarea)) => overview.rows.push(Row {
            line: Line::raw(""),
            stops: Vec::new(),
            input: input_rows(overview.width, textarea, false),
        }),
        _ if overview.pr.body.trim().is_empty() => {
            overview
                .rows
                .push(Row::stop(Line::styled("  0 description", theme::muted()), Stop::Body));
        }
        _ => {
            for rendered in
                crate::ui::markdown::markdown(&overview.pr.body, overview.width.saturating_sub(2), &[Span::raw("  ")])
            {
                let width = rendered.line.width() as u16;

                overview.rows.push(Row {
                    line: rendered.line,
                    stops: vec![(0, width, Stop::Body)],
                    input: 0,
                });
            }
        }
    }
}

struct CommentBox {
    index: usize,
    border: Style,
    on_line: String,
    state: &'static str,
}

fn append_new_comment(overview: &mut OverviewRows<'_>) {
    match overview.edit {
        Some((Edit::Comment, textarea)) => {
            overview.rows.push(Row::stop(
                Line::styled(
                    "  + write a comment",
                    highlight(overview.focus, Stop::NewComment, theme::accent()),
                ),
                Stop::NewComment,
            ));
            overview.rows.push(Row {
                line: Line::raw(""),
                stops: Vec::new(),
                input: input_rows(overview.width, textarea, false),
            });
        }
        _ => overview.rows.push(Row::stop(
            Line::styled(
                "  + write a comment",
                highlight(overview.focus, Stop::NewComment, theme::muted()),
            ),
            Stop::NewComment,
        )),
    }
}

fn comment_box(overview: &OverviewRows<'_>, index: usize, thread: &gh::Thread) -> CommentBox {
    // the comment box with the focus: an accent border (as a picked file's box)
    let picked = matches!(overview.focus, Some(Stop::Comment(focused, _)) if focused == index);

    let border = if picked {
        theme::accent_bold()
    } else {
        Style::default().fg(theme::pal().overlay0)
    };

    let on_line = thread
        .path
        .as_ref()
        .map(|path| format!(" · {path}:{}", thread.line.unwrap_or(0)))
        .unwrap_or_default();

    let state = if thread.resolved {
        " · resolved"
    } else if thread.outdated {
        " · outdated"
    } else {
        ""
    };

    CommentBox {
        index,
        border,
        on_line,
        state,
    }
}

fn append_thread(overview: &mut OverviewRows<'_>, index: usize, thread: &gh::Thread) {
    let context = comment_box(overview, index, thread);
    let top = overview.rows.len();

    for (position, comment) in std::iter::once(&thread.root).chain(&thread.replies).enumerate() {
        append_comment(overview, &context, position, comment);
    }

    if let Some((Edit::Reply(reply_thread, _), textarea)) = overview.edit
        && *reply_thread == index
    {
        overview.rows.push(Row {
            line: Line::raw(""),
            stops: Vec::new(),
            input: input_rows(overview.width, textarea, false),
        });
    }

    // the bottom line: as wide as the box's widest row (a table), min. 40
    let widest = overview.rows[top..]
        .iter()
        .map(|row| row.line.width())
        .max()
        .unwrap_or(0);

    overview.rows.push(Row::new(Line::styled(
        format!(
            "╰{}",
            "─".repeat(widest.saturating_sub(1).max(40).min(overview.width.saturating_sub(1)))
        ),
        context.border,
    )));
}

fn comment_heading(comment: &gh::Comment, position: usize, context: &CommentBox) -> String {
    let on_line = &context.on_line;
    let state = context.state;

    let verdict = match comment.verdict.as_str() {
        "APPROVED" => " · approved",
        "CHANGES_REQUESTED" => " · requested changes",
        _ => "",
    };

    let who = if comment.mine {
        format!("{} (you)", comment.author)
    } else {
        comment.author.clone()
    };

    if position == 0 {
        format!("{who} · {}{verdict}{on_line}{state}", gh::stamp(comment.created))
    } else {
        format!("↳ {who} · {}", gh::stamp(comment.created))
    }
}

fn append_comment(overview: &mut OverviewRows<'_>, context: &CommentBox, position: usize, comment: &gh::Comment) {
    let stop = Stop::Comment(context.index, position);

    let head = comment_heading(comment, position, context);

    let indent = append_comment_header(overview, context, position, comment, stop, &head);
    let editing = matches!(overview.edit, Some((Edit::Change(edit_thread, edit_position, ..), _)) if *edit_thread == context.index && *edit_position == position);

    if editing {
        append_edit_input(overview);
        return;
    }

    let prefix = [Span::styled(indent, context.border)];

    for rendered in crate::ui::markdown::markdown(
        &comment.body,
        overview.width.saturating_sub(indent.width() + 1),
        &prefix,
    ) {
        overview.rows.push(Row::new(rendered.line));
    }
}

fn append_comment_header(
    overview: &mut OverviewRows<'_>,
    context: &CommentBox,
    position: usize,
    comment: &gh::Comment,
    stop: Stop,
    head: &str,
) -> &'static str {
    let (lead, indent) = if position == 0 {
        ("╭─ ", "│ ")
    } else {
        ("│  ", "│    ")
    };

    let hint = if overview.focus == Some(stop) {
        comment_hint(comment)
    } else {
        ""
    };

    let name_style = highlight(
        overview.focus,
        stop,
        Style::default()
            .fg(if comment.mine {
                theme::pal().info()
            } else {
                theme::pal().text
            })
            .add_modifier(Modifier::BOLD),
    );
    let line = Line::from(vec![
        Span::styled(lead, context.border),
        Span::styled(
            crate::ui::thread_card::fit(head, overview.width.saturating_sub(lead.width() + hint.width())),
            name_style,
        ),
        Span::styled(hint, theme::muted()),
    ]);
    let width = line.width() as u16;

    overview.rows.push(Row {
        line,
        stops: vec![(0, width, stop)],
        input: 0,
    });

    indent
}
