use super::{
    Body, CLine, Cat, FileDiff, Kind, Line, Modifier, Row, Span, Status, Style, UnicodeWidthStr, View, gh_lines,
    painted_spans, scm, sign_span, stamp, theme, threads,
};

impl View {
    /// The line of row I in the built width.
    pub(super) fn line(&self, i: usize) -> Line<'static> {
        line_view(self, i)
    }
}

fn head_details(file: &FileDiff) -> (String, usize, String) {
    let (name, skip) = match &file.old_path {
        Some(old) => (format!("{old} → {}", file.path), old.chars().count() + 3),
        None => (file.path.clone(), 0),
    };

    let tag = match file.status {
        Status::New => " new",
        Status::Deleted => " deleted",
        _ => "",
    };

    let right = if file.binary {
        "binary".to_string()
    } else {
        format!("+{} −{}", file.add, file.del)
    };

    let right = format!(" {}{tag} · {right} ", file.cat.label());

    (name, skip, right)
}

fn hunk_line(
    inner: usize,
    bar: impl Fn(Vec<Span<'static>>) -> Line<'static>,
    file: &FileDiff,
    hunk_index: usize,
) -> Line<'static> {
    let hunk = &file.hunks[hunk_index];
    let label = format!(
        "⋯ {}",
        if hunk.ctx.is_empty() {
            format!("line {}", hunk.new)
        } else {
            hunk.ctx.clone()
        }
    );

    bar(vec![Span::styled(
        crate::ui::thread_card::fit(&label, inner),
        theme::muted(),
    )])
}

fn conflict_line(width: usize, conflicts: &[String]) -> Line<'static> {
    let files = if conflicts.is_empty() {
        "0 conflicts".to_string()
    } else {
        format!("{} conflicts: {}", conflicts.len(), conflicts.join(", "))
    };

    Line::from(vec![
        Span::styled(" rebase stopped ", theme::pill()),
        Span::styled(
            crate::ui::thread_card::fit(
                &format!(" {files} · S: the agents resolve · continue · abort"),
                width.saturating_sub(16),
            ),
            theme::fg(theme::pal().attention()),
        ),
    ])
}

fn comment_line(
    view: &View,
    inner: usize,
    bar: impl Fn(Vec<Span<'static>>) -> Line<'static>,
    ci: usize,
    line: usize,
) -> Line<'static> {
    let Some(comment) = view.comment_state.comments.get(ci) else {
        return bar(Vec::new());
    };

    let style = theme::fg(theme::pal().progress());

    let text = |text_line: &str, indent: usize, style: Style| {
        bar(vec![
            Span::raw(" ".repeat(indent)),
            Span::styled(
                crate::ui::thread_card::fit(text_line, inner.saturating_sub(indent)),
                style,
            ),
        ])
    };

    match view.clines(comment).get(line).copied().unwrap_or(CLine::Head) {
        CLine::Text(j) => return text(comment.text.lines().nth(j).unwrap_or_default(), 2, style),
        CLine::RText(reply, j) => return reply_text_line(comment, text, reply, j),
        CLine::Reply(reply) => return reply_head_line(inner, &bar, comment, reply),
        CLine::Head => {}
    }

    // the 1st row: picked mark, severity, id, lines, author (you / @agent), sent; folded:
    // the count of the lines and replies out of view, the unseen replies
    comment_head_line(view, inner, bar, comment, style)
}

fn comment_head_line(
    view: &View,
    inner: usize,
    bar: impl Fn(Vec<Span<'static>>) -> Line<'static>,
    comment: &super::Comment,
    style: Style,
) -> Line<'static> {
    let pick = if view.scm.comment_picks.contains(&comment.id) {
        "● "
    } else {
        ""
    };

    let sev = format!("{pick}{} ", comment.level.label());
    let sent = if comment.sent { " · sent" } else { "" };
    let head = format!("✎ {} {} · {}{sent}: ", comment.id, comment.at(), comment.author());
    let more = comment.text.lines().count().saturating_sub(1);
    let mut hidden = Vec::new();

    if view.comment_shut(comment) && more > 0 {
        hidden.push(format!("+{more} lines"));
    }

    if view.comment_shut(comment) && !comment.replies.is_empty() {
        hidden.push(format!(
            "{} repl{}",
            comment.replies.len(),
            if comment.replies.len() == 1 { "y" } else { "ies" }
        ));
    }

    let tail = if hidden.is_empty() {
        String::new()
    } else {
        format!(" ({})", hidden.join(" · "))
    };

    let new = threads::unseen(comment);

    let fresh = if new > 0 && view.comment_shut(comment) {
        format!(" ● {new} new")
    } else {
        String::new()
    };

    let room = inner.saturating_sub(head.width() + sev.width() + tail.width() + fresh.width());

    bar(vec![
        Span::styled(
            sev,
            Style::default()
                .fg(scm::level_color(comment.level))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(head, style.add_modifier(Modifier::BOLD)),
        Span::styled(
            crate::ui::thread_card::fit(comment.text.lines().next().unwrap_or_default(), room),
            style,
        ),
        Span::styled(tail, theme::muted()),
        Span::styled(fresh, theme::accent_bold()),
    ])
}

fn reply_head_line(
    inner: usize,
    bar: &impl Fn(Vec<Span<'static>>) -> Line<'static>,
    comment: &super::Comment,
    index: usize,
) -> Line<'static> {
    let Some(reply) = comment.replies.get(index) else {
        return bar(Vec::new());
    };

    let dot = if reply.unseen { "● " } else { "" };
    let head = format!("  ↳ {dot}{} · {}: ", reply.author(), stamp(reply.at));
    let room = inner.saturating_sub(head.width());

    let style = if reply.unseen {
        theme::accent_bold()
    } else {
        theme::bold()
    };

    bar(vec![
        Span::styled(head, style),
        Span::styled(
            crate::ui::thread_card::fit(reply.text.lines().next().unwrap_or_default(), room),
            theme::text(),
        ),
    ])
}

fn reply_text_line(
    comment: &super::Comment,
    text: impl Fn(&str, usize, Style) -> Line<'static>,
    index: usize,
    line: usize,
) -> Line<'static> {
    text(
        comment
            .replies
            .get(index)
            .and_then(|reply| reply.text.lines().nth(line))
            .unwrap_or_default(),
        6,
        theme::text(),
    )
}

fn review_thread_line(
    view: &View,
    inner: usize,
    bar: impl Fn(Vec<Span<'static>>) -> Line<'static>,
    ti: usize,
    line_index: usize,
) -> Line<'static> {
    let Some(thread) = view.pr_src.as_ref().and_then(|src| src.threads.get(ti)) else {
        return bar(Vec::new());
    };

    let lines = gh_lines(thread, inner);

    let Some((head, text)) = lines.get(line_index) else {
        return bar(Vec::new());
    };

    let style = theme::fg(theme::pal().progress());

    if *head {
        bar(vec![Span::styled(
            crate::ui::thread_card::fit(text, inner),
            style.add_modifier(Modifier::BOLD),
        )])
    } else {
        bar(vec![Span::styled(crate::ui::thread_card::fit(text, inner), style)])
    }
}

fn finding_line(
    view: &View,
    inner: usize,
    bar: impl Fn(Vec<Span<'static>>) -> Line<'static>,
    fi: usize,
    line_index: usize,
) -> Line<'static> {
    let Some(finding) = view.reviewer.review.as_ref().and_then(|review| review.findings.get(fi)) else {
        return bar(Vec::new());
    };

    let color = match finding.severity.as_str() {
        "critical" => theme::pal().error(),
        "major" => theme::pal().caution(),
        _ => theme::pal().subtext0,
    };

    let style = Style::default().fg(color).add_modifier(Modifier::BOLD);

    if line_index == 0 {
        let head = format!("⚠ {} · ", finding.severity);

        bar(vec![
            Span::styled(head.clone(), style),
            Span::styled(
                crate::ui::thread_card::fit(&finding.title, inner.saturating_sub(head.width())),
                style,
            ),
        ])
    } else {
        let line = finding.body.lines().nth(line_index - 1).unwrap_or_default();

        bar(vec![
            Span::raw("  "),
            Span::styled(
                crate::ui::thread_card::fit(line, inner.saturating_sub(2)),
                theme::muted(),
            ),
        ])
    }
}

fn side_line(
    view: &View,
    row: &Row,
    inner: usize,
    bar: impl Fn(Vec<Span<'static>>) -> Line<'static>,
    file: &FileDiff,
    hunk_index: usize,
    columns: (Option<usize>, Option<usize>),
) -> Line<'static> {
    let (lines, painted) = (&file.hunks[hunk_index].lines, view.painted(file, hunk_index));
    let num_w = view.layout.num_w.get(&row.file).copied().unwrap_or(3);
    let half = inner.saturating_sub(3) / 2;

    let cell = |line_index: Option<usize>, left: bool| -> Vec<Span<'static>> {
        let Some(line_index) = line_index else {
            return vec![Span::raw(" ".repeat(half))];
        };

        let line = &lines[line_index];
        let number = if left { line.old } else { line.new }.unwrap_or(0);
        let mut spans = vec![
            Span::styled(format!("{number:>num_w$}"), theme::muted()),
            sign_span(line.kind),
        ];

        if let Some(painted_line) = painted.get(line_index) {
            spans.extend(painted_spans(painted_line, line.kind, half.saturating_sub(num_w + 3)));
        }

        spans
    };

    let mut text = cell(columns.0, true);

    text.push(Span::styled(" │ ", theme::line()));
    text.extend(cell(columns.1, false));

    bar(text)
}

fn group_line(view: &View, width: usize, group: Option<usize>, cat: Cat) -> Line<'static> {
    match group.and_then(|group| {
        view.classifier
            .rank
            .as_ref()
            .and_then(|rank| rank.groups.get(group))
            .map(|ranked| (group, ranked))
    }) {
        Some((index, ranked)) => Line::from(vec![
            Span::styled(format!(" {} · {}", index + 1, ranked.title), theme::accent_bold()),
            Span::styled(
                crate::ui::thread_card::fit(
                    &format!("  {}", ranked.why),
                    width.saturating_sub(ranked.title.width() + 8),
                ),
                theme::muted(),
            ),
        ]),
        None => Line::styled(format!(" {}", cat.label()), theme::muted().add_modifier(Modifier::BOLD)),
    }
}

fn diff_line(
    view: &View,
    row: &Row,
    inner: usize,
    bar: impl Fn(Vec<Span<'static>>) -> Line<'static>,
    file: &FileDiff,
    hunk_index: usize,
    line_index: usize,
) -> Line<'static> {
    let (line, painted) = (
        &file.hunks[hunk_index].lines[line_index],
        view.painted(file, hunk_index),
    );
    let num_w = view.layout.num_w.get(&row.file).copied().unwrap_or(3);

    let num = match line.kind {
        Kind::Add => format!("{:>num_w$} {:>num_w$}", "", line.new.unwrap_or(0)),
        Kind::Del => format!("{:>num_w$} {:>num_w$}", line.old.unwrap_or(0), ""),
        Kind::Ctx => format!("{:>num_w$} {:>num_w$}", line.old.unwrap_or(0), line.new.unwrap_or(0)),
    };

    let room = inner.saturating_sub(num.len() + 3);
    let mut spans = vec![Span::styled(num, theme::muted()), sign_span(line.kind)];

    if let Some(painted_line) = painted.get(line_index) {
        spans.extend(painted_spans(painted_line, line.kind, room));
    }

    bar(spans)
}

/// The border style of file I's box: accent on the file of the cursor.
/// The panel's keys on a file row: that file.
fn border(i: usize, view: &View) -> Style {
    let on = match view.scm.file_at() {
        Some(file) => file == i,
        None => view.layout.rows.get(view.at).is_some_and(|row| {
            row.file == i && !matches!(row.body, Body::Blank | Body::Group(..) | Body::Hits(_) | Body::Conflict)
        }),
    };

    if on { theme::accent() } else { theme::line() }
}

/// The head row of file I (FILE): reviewed mark, staged mark, path (filter hits marked), level,
/// category, size.
fn head_line(i: usize, file: &FileDiff, width: usize, view: &View) -> Line<'static> {
    let reviewed = view.is_reviewed(file);
    let updated = !reviewed && view.marks.contains_key(&file.path);
    let border = border(i, view);

    let mark = if reviewed {
        Span::styled("☑ ", Style::default().fg(theme::pal().success()))
    } else {
        Span::styled("☐ ", theme::muted())
    };

    let (name, skip, right) = head_details(file);
    let room = width.saturating_sub(right.width() + 8 + if updated { 9 } else { 0 });

    let stage = match view.changes.staged.get(&file.path) {
        Some(true) => Span::styled("● ", Style::default().fg(theme::pal().success())),
        Some(false) => Span::styled("◐ ", theme::fg(theme::pal().info())),
        None => Span::raw(""),
    };

    // a folded box: 1 line, 0 corners
    let (open, close) = if view.folded(file) {
        ("── ", "──")
    } else {
        ("┌─ ", "─┐")
    };

    let mut head = vec![Span::styled(open, border), mark, stage];
    let fitted = crate::ui::thread_card::fit(&name, room);

    let marks: Vec<usize> = view
        .filter_state
        .hits
        .get(&i)
        .map(|hits| {
            hits.iter()
                .map(|hit| hit + skip)
                .filter(|&hit| hit < fitted.chars().count())
                .collect()
        })
        .unwrap_or_default();

    head.extend(crate::ui::fuzzy::spans(
        &fitted,
        &marks,
        theme::bold(),
        theme::accent_bold().add_modifier(Modifier::UNDERLINED),
    ));

    if updated {
        head.push(Span::styled(" updated", theme::fg(theme::pal().attention())));
    }

    let used: usize = head.iter().map(|span| span.content.width()).sum();
    let padding = width.saturating_sub(used + right.width() + 3);

    head.push(Span::styled(format!(" {}", "─".repeat(padding)), border));
    head.push(Span::styled(right, theme::muted()));
    head.push(Span::styled(close, border));

    Line::from(head)
}

fn line_view(view: &View, i: usize) -> Line<'static> {
    let Some(row) = view.layout.rows.get(i) else {
        return Line::raw("");
    };

    let width = usize::from(view.layout.built.map_or(80, |built| built.0));
    let inner = width.saturating_sub(4);
    let border = border(row.file, view);

    let bar = |mut spans: Vec<Span<'static>>| -> Line<'static> {
        let used: usize = spans.iter().map(|span| span.content.width()).sum();
        let mut line = vec![Span::styled("│ ", border)];

        line.append(&mut spans);
        line.push(Span::raw(" ".repeat(inner.saturating_sub(used))));
        line.push(Span::styled(" │", border));
        Line::from(line)
    };

    let Some(file) = view.changes.files.get(row.file) else {
        return Line::raw("");
    };

    match row.body {
        Body::Conflict => {
            let conflicts = view.changes.stopped.clone().unwrap_or_default();

            conflict_line(width, &conflicts)
        }
        Body::Blank => Line::raw(""),
        Body::Group(group, cat) => group_line(view, width, group, cat),
        Body::Hits(count) => Line::styled(
            format!(" {count} files match · by name and content, every level · enter: the 1st · esc: clear"),
            theme::muted(),
        ),
        Body::Head => head_line(row.file, file, width, view),
        Body::Foot => Line::styled(format!("└{}┘", "─".repeat(width.saturating_sub(2))), border),
        Body::Why => {
            let why = view.ranked(&file.path).map(|(_, _, why)| why).unwrap_or_default();

            bar(vec![Span::styled(
                crate::ui::thread_card::fit(why, inner),
                theme::muted().add_modifier(Modifier::ITALIC),
            )])
        }
        Body::Note(note) => bar(vec![Span::styled(note.to_string(), theme::muted())]),
        Body::Hunk(hunk_index) => hunk_line(inner, bar, file, hunk_index),
        Body::Diff(hunk_index, line_index) => diff_line(view, row, inner, bar, file, hunk_index, line_index),
        Body::Side(hunk_index, removed, added) => side_line(view, row, inner, bar, file, hunk_index, (removed, added)),
        Body::Finding(fi, line_index) => finding_line(view, inner, bar, fi, line_index),
        Body::Gh(ti, line_index) => review_thread_line(view, inner, bar, ti, line_index),
        Body::Comment(ci, line) => comment_line(view, inner, bar, ci, line),
    }
}
