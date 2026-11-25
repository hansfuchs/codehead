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

fn comment_hint(character_5: &gh::Comment) -> &'static str {
    if character_5.mine {
        "  enter: edit / delete"
    } else if character_5.kind == Kind::Line {
        "  enter: reply"
    } else {
        "  enter: quote in a reply"
    }
}

fn append_edit_input(overview: &mut OverviewRows<'_>) {
    if let Some((_, ta)) = overview.edit {
        overview.rows.push(Row {
            line: Line::raw(""),
            stops: Vec::new(),
            input: input_rows(overview.width, ta, false),
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
        Some((Edit::Body, ta)) => overview.rows.push(Row {
            line: Line::raw(""),
            stops: Vec::new(),
            input: input_rows(overview.width, ta, false),
        }),
        _ if overview.pr.body.trim().is_empty() => {
            overview
                .rows
                .push(Row::stop(Line::styled("  0 description", theme::muted()), Stop::Body));
        }
        _ => {
            for result_3 in
                crate::ui::markdown::markdown(&overview.pr.body, overview.width.saturating_sub(2), &[Span::raw("  ")])
            {
                let lw = result_3.line.width() as u16;

                overview.rows.push(Row {
                    line: result_3.line,
                    stops: vec![(0, lw, Stop::Body)],
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
        Some((Edit::Comment, ta)) => {
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
                input: input_rows(overview.width, ta, false),
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
    let picked = matches!(overview.focus, Some(Stop::Comment(fi, _)) if fi == index);

    let line_c = if picked {
        theme::accent_bold()
    } else {
        Style::default().fg(theme::pal().overlay0)
    };

    let on_line = thread
        .path
        .as_ref()
        .map(|pa| format!(" · {pa}:{}", thread.line.unwrap_or(0)))
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
        border: line_c,
        on_line,
        state,
    }
}

fn append_thread(overview: &mut OverviewRows<'_>, i: usize, th: &gh::Thread) {
    let context = comment_box(overview, i, th);
    let top = overview.rows.len();

    for (key, comment) in std::iter::once(&th.root).chain(&th.replies).enumerate() {
        append_comment(overview, &context, key, comment);
    }

    if let Some((Edit::Reply(ti, _), ta)) = overview.edit
        && *ti == i
    {
        overview.rows.push(Row {
            line: Line::raw(""),
            stops: Vec::new(),
            input: input_rows(overview.width, ta, false),
        });
    }

    // the bottom line: as wide as the box's widest row (a table), min. 40
    let widest = overview.rows[top..]
        .iter()
        .map(|result_2| result_2.line.width())
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

fn comment_heading(character_5: &gh::Comment, key: usize, context: &CommentBox) -> String {
    let on_line = &context.on_line;
    let state = context.state;

    let verdict = match character_5.verdict.as_str() {
        "APPROVED" => " · approved",
        "CHANGES_REQUESTED" => " · requested changes",
        _ => "",
    };

    let who = if character_5.mine {
        format!("{} (you)", character_5.author)
    } else {
        character_5.author.clone()
    };

    if key == 0 {
        format!("{who} · {}{verdict}{on_line}{state}", gh::stamp(character_5.created))
    } else {
        format!("↳ {who} · {}", gh::stamp(character_5.created))
    }
}

fn append_comment(overview: &mut OverviewRows<'_>, context: &CommentBox, key: usize, character_5: &gh::Comment) {
    let comment = Stop::Comment(context.index, key);

    let head = comment_heading(character_5, key, context);

    let indent = append_comment_header(overview, context, key, character_5, comment, &head);
    let editing = matches!(overview.edit, Some((Edit::Change(ti, tk, ..), _)) if *ti == context.index && *tk == key);

    if editing {
        append_edit_input(overview);
        return;
    }

    let prefix = [Span::styled(indent, context.border)];

    for result_value in crate::ui::markdown::markdown(
        &character_5.body,
        overview.width.saturating_sub(indent.width() + 1),
        &prefix,
    ) {
        overview.rows.push(Row::new(result_value.line));
    }
}

fn append_comment_header(
    overview: &mut OverviewRows<'_>,
    context: &CommentBox,
    key: usize,
    character_5: &gh::Comment,
    comment: Stop,
    head: &str,
) -> &'static str {
    let (lead, indent) = if key == 0 {
        ("╭─ ", "│ ")
    } else {
        ("│  ", "│    ")
    };

    let hint = if overview.focus == Some(comment) {
        comment_hint(character_5)
    } else {
        ""
    };

    let name_st = highlight(
        overview.focus,
        comment,
        Style::default()
            .fg(if character_5.mine {
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
            name_st,
        ),
        Span::styled(hint, theme::muted()),
    ]);
    let lw = line.width() as u16;

    overview.rows.push(Row {
        line,
        stops: vec![(0, lw, comment)],
        input: 0,
    });

    indent
}
