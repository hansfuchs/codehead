use super::{
    Btn, Color, Fold, Level, Line, Modifier, PRow, Rect, Sec, Span, Style, UnicodeWidthStr, View, level_color,
    split_path, status_letter, theme,
};

impl View {
    /// The line of panel row ROW in WIDTH columns (its buttons' click targets in RECT).
    pub(super) fn panel_line(&mut self, row: &PRow, width: usize, rect: Rect) -> Line<'static> {
        panel_line_view(self, row, width, rect)
    }
}

fn panel_line_pick(
    view: &mut View,
    width: usize,
    right_fill: impl Fn(&[Span<'static>], &[Span<'static>]) -> Line<'static>,
) -> Line<'static> {
    let (text, detail) = view.pick_label(view.pick_index());
    let left = vec![
        Span::styled(" ◆ ", theme::accent()),
        Span::styled(
            crate::ui::thread_card::fit(&text, width.saturating_sub(detail.width() + 8)),
            theme::bold(),
        ),
    ];

    right_fill(&left, &[Span::styled(format!("{detail} ▾ "), theme::muted())])
}

fn panel_line_commit_file(
    view: &mut View,
    width: usize,
    right_fill: impl Fn(&[Span<'static>], &[Span<'static>]) -> Line<'static>,
    fi: usize,
) -> Line<'static> {
    let Some(peek) = view.peek.as_ref() else {
        return Line::raw("");
    };

    let Some(file) = peek.changes.files.get(fi) else {
        return Line::raw("");
    };

    let here = peek.layout.rows.get(peek.at).is_some_and(|row| row.file == fi);
    let (letter, color) = status_letter(file.status);
    let (name, dir) = split_path(&file.path);
    let indent = "       ";
    let room = width.saturating_sub(indent.len() + 3);
    let name = crate::ui::thread_card::fit(name, room);
    let dir_room = room.saturating_sub(name.width() + 1);
    let mut left = vec![
        Span::raw(indent),
        Span::styled(name, if here { theme::accent_bold() } else { theme::text() }),
    ];

    if dir_room > 3 && !dir.is_empty() {
        left.push(Span::styled(
            format!(" {}", crate::ui::thread_card::fit(dir, dir_room)),
            theme::muted(),
        ));
    }

    right_fill(
        &left,
        &[Span::styled(
            format!("{letter} "),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )],
    )
}

fn panel_line_commit(
    view: &mut View,
    width: usize,
    right_fill: impl Fn(&[Span<'static>], &[Span<'static>]) -> Line<'static>,
    ci: usize,
) -> Line<'static> {
    let Some(commit) = view.changes.commits.list.get(ci) else {
        return Line::raw("");
    };

    let shown = view.peeked() == Some(commit.oid.as_str());

    let (dot, dot_st) = match (shown, commit.pushed) {
        (true, _) => ("◆", theme::accent()),
        (false, false) => ("●", theme::fg(theme::pal().attention())),
        (false, true) => ("○", theme::muted()),
    };

    let age = crate::ui::thread_card::ago(commit.time.max(0) as u64 * 1000).replace(" ago", "");
    let right = vec![Span::styled(format!("{age} "), theme::muted())];
    let room = width.saturating_sub(5 + commit.short.len() + 1 + age.width() + 2);
    let subject = crate::ui::thread_card::fit(&commit.subject, room);

    let sub_st = if commit.pushed { theme::muted() } else { theme::text() };

    let left = vec![
        Span::raw("     "),
        Span::styled(format!("{dot} "), dot_st),
        Span::styled(format!("{} ", commit.short), theme::muted()),
        Span::styled(subject, sub_st),
    ];

    right_fill(&left, &right)
}

fn panel_line_all(
    view: &mut View,
    width: usize,
    right_fill: impl Fn(&[Span<'static>], &[Span<'static>]) -> Line<'static>,
) -> Line<'static> {
    let on = view.peek.as_ref().is_some_and(|peek| peek.all);

    let (dot, st) = if on {
        ("◆", theme::accent())
    } else {
        ("◇", theme::muted())
    };

    let commits = view.changes.commits.list.len();
    let left = vec![
        Span::styled(format!("   {dot} "), st),
        Span::styled("all changes", theme::text()),
    ];

    right_fill(
        &left,
        &[Span::styled(
            crate::ui::thread_card::fit(
                &format!("{commits} commits + uncommitted · vs {} ", view.changes.base),
                width.saturating_sub(16),
            ),
            theme::muted(),
        )],
    )
}

fn panel_line_pushing_commit(
    view: &mut View,
    width: usize,
    right_fill: impl Fn(&[Span<'static>], &[Span<'static>]) -> Line<'static>,
    ci: usize,
) -> Line<'static> {
    let Some(commit) = view.changes.commits.list.get(ci) else {
        return Line::raw("");
    };

    let room = width.saturating_sub(5 + commit.short.len() + 3);
    let left = vec![
        Span::raw("     "),
        Span::styled("↑ ", theme::accent()),
        Span::styled(format!("{} ", commit.short), theme::muted()),
        Span::styled(crate::ui::thread_card::fit(&commit.subject, room), theme::muted()),
    ];

    right_fill(&left, &[])
}

fn panel_line_pushing(
    view: &mut View,
    right_fill: impl Fn(&[Span<'static>], &[Span<'static>]) -> Line<'static>,
    count: usize,
) -> Line<'static> {
    let st = theme::accent_bold();

    let tick = view.pushing_at.map_or(0, |started| {
        (started.elapsed().as_millis() / crate::ui::thread_card::WAVE_MS) as usize
    });

    let wave = crate::ui::thread_card::WAVE[tick % crate::ui::thread_card::WAVE.len()];

    // another process's push: who (an agent, shell)
    let by = view.push_by.as_ref().map(|who| format!(" · {who}")).unwrap_or_default();

    right_fill(
        &[
            Span::styled("   PUSHING", st),
            Span::styled(format!("  {count}{by}"), theme::muted()),
        ],
        &[Span::styled(wave, theme::accent())],
    )
}

fn panel_line_push(
    view: &mut View,
    width: usize,
    rect: Rect,
    indicator: &'static str,
    right_fill: impl Fn(&[Span<'static>], &[Span<'static>]) -> Line<'static>,
    pushed: bool,
    count: usize,
) -> Line<'static> {
    let (label, color) = if pushed {
        ("PUSHED", theme::pal().subtext0)
    } else {
        ("UNPUSHED", theme::pal().attention())
    };

    let st = Style::default().fg(color).add_modifier(Modifier::BOLD);
    let cs = &view.changes.commits;
    let mut right = format!("{count} ");

    if pushed && let Some(upstream) = &cs.upstream {
        right = format!("{count} · {upstream} ");
    }

    if !pushed && cs.remote_only > 0 {
        right = format!("{count} · ↓{} on the remote ", cs.remote_only);
    }

    let mut spans = vec![Span::styled(
        crate::ui::thread_card::fit(&right, width / 2),
        theme::muted(),
    )];

    // unpushed, 0 remote-only commits: a push button (P)
    if !pushed && cs.remote_only == 0 && count > 0 {
        let label = "↑ push ";
        let bw = label.width() as u16;

        view.scm.buttons.push((
            Rect {
                x: rect.right().saturating_sub(bw),
                width: bw,
                ..rect
            },
            Btn::Push,
        ));

        // a push runs: grayed (a click toasts why)
        spans.push(Span::styled(
            label,
            if view.pushing() {
                theme::muted()
            } else {
                theme::accent()
            },
        ));
    }

    right_fill(&[Span::styled(format!("   {indicator} {label}"), st)], &spans)
}

fn panel_line_stash(
    view: &mut View,
    width: usize,
    rect: Rect,
    right_fill: impl Fn(&[Span<'static>], &[Span<'static>]) -> Line<'static>,
    si: usize,
) -> Line<'static> {
    let Some(entry) = view.stash_state.stash.get(si).cloned() else {
        return Line::raw("");
    };

    let mut right = Vec::new();

    if entry.restoring {
        right.push(Span::styled("restoring… ", theme::muted()));
    } else {
        let (rl, dl) = ("↺ ", "✕ ");
        let x0 = rect.right().saturating_sub(5);

        view.scm.buttons.push((
            Rect {
                x: x0,
                width: 2,
                ..rect
            },
            Btn::Restore(entry.id.clone()),
        ));
        view.scm.buttons.push((
            Rect {
                x: x0 + 3,
                width: 2,
                ..rect
            },
            Btn::Drop(entry.id.clone()),
        ));
        right.push(Span::styled(rl, theme::accent()));
        right.push(Span::raw(" "));
        right.push(Span::styled(dl, theme::muted()));
    }

    let room = width.saturating_sub(5 + 5 + if entry.restoring { 6 } else { 0 });
    let title = crate::ui::thread_card::fit(&entry.title(), room);
    let remaining = room.saturating_sub(title.width() + 1);
    let mut left = vec![
        Span::styled("   ⧉ ", theme::muted()),
        Span::styled(
            title,
            if entry.name.is_empty() {
                theme::muted().add_modifier(Modifier::ITALIC)
            } else {
                theme::text()
            },
        ),
    ];

    if !entry.name.is_empty() && remaining > 4 {
        left.push(Span::styled(
            format!(" {}", crate::ui::thread_card::fit(&entry.spot(), remaining)),
            theme::muted(),
        ));
    }

    right_fill(&left, &right)
}

fn panel_line_file(
    view: &mut View,
    width: usize,
    right_fill: impl Fn(&[Span<'static>], &[Span<'static>]) -> Line<'static>,
    i: usize,
    staged: bool,
) -> Line<'static> {
    let Some(file) = view.changes.files.get(i) else {
        return Line::raw("");
    };

    let (letter, color) = letter(file, staged, view);
    let reviewed = view.is_reviewed(file);
    let (name, dir) = split_path(&file.path);
    let indent = if staged { "   " } else { "     " };
    let right = vec![
        Span::styled(
            if reviewed { "✓ " } else { "  " },
            Style::default().fg(theme::pal().success()),
        ),
        Span::styled(
            format!("{letter} "),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
    ];
    let room = width.saturating_sub(indent.len() + 5);
    let name_st = if reviewed { theme::muted() } else { theme::text() };
    let name = crate::ui::thread_card::fit(name, room);
    let dir_room = room.saturating_sub(name.width() + 1);
    let mut left = vec![Span::raw(indent), Span::styled(name, name_st)];

    if dir_room > 3 && !dir.is_empty() {
        left.push(Span::styled(
            format!(" {}", crate::ui::thread_card::fit(dir, dir_room)),
            theme::muted(),
        ));
    }

    right_fill(&left, &right)
}

fn panel_line_level(
    view: &mut View,
    indicator: &'static str,
    right_fill: impl Fn(&[Span<'static>], &[Span<'static>]) -> Line<'static>,
    level: Level,
    count: usize,
) -> Line<'static> {
    let (done, all) = view.classifier.counts[level as usize];
    let st = Style::default().fg(level_color(level)).add_modifier(Modifier::BOLD);

    right_fill(
        &[Span::styled(format!("   {} {}", indicator, level.label()), st)],
        &[Span::styled(format!("{count} · {done}/{all} ✓ "), theme::muted())],
    )
}

fn panel_line_section(
    view: &mut View,
    rect: Rect,
    indicator: &'static str,
    right_fill: impl Fn(&[Span<'static>], &[Span<'static>]) -> Line<'static>,
    sec: Sec,
    count: usize,
) -> Line<'static> {
    let left = vec![
        Span::styled(format!(" {} {}", indicator, sec.label()), theme::bold()),
        Span::styled(format!("  {count}"), theme::muted()),
    ];
    let mut right = Vec::new();

    if sec == Sec::Stash && count > 0 {
        let label = "✕ clear ";
        let bw = label.width() as u16;

        view.scm.buttons.push((
            Rect {
                x: rect.right().saturating_sub(bw),
                width: bw,
                ..rect
            },
            Btn::Clear,
        ));
        right.push(Span::styled(label, theme::muted()));
    }

    right_fill(&left, &right)
}

/// VS Code's 1-letter status of FILE: U untracked, A added, M modified, D deleted, R renamed.
fn letter(file: &crate::features::changes::FileDiff, staged: bool, view: &View) -> (char, Color) {
    if !staged && view.changes.untracked.contains(&file.path) {
        return ('U', theme::pal().success());
    }

    status_letter(file.status)
}

fn panel_line_view(view: &mut View, row: &PRow, width: usize, rect: Rect) -> Line<'static> {
    let fold = |group: Fold| {
        if view.scm.folded.contains(&group) { "▸" } else { "▾" }
    };

    let right_fill = |left: &[Span<'static>], right: &[Span<'static>]| -> Line<'static> {
        let lw: usize = left.iter().map(|span| span.content.width()).sum();
        let rw: usize = right.iter().map(|span| span.content.width()).sum();
        let mut spans = left.to_vec();

        spans.push(Span::raw(" ".repeat(width.saturating_sub(lw + rw).max(1))));
        spans.extend(right.iter().cloned());
        Line::from(spans)
    };

    match row {
        PRow::Section(sec, count) => panel_line_section(view, rect, fold(Fold::Sec(*sec)), right_fill, *sec, *count),
        PRow::Level(level, count) => panel_line_level(view, fold(Fold::Level(*level)), right_fill, *level, *count),
        PRow::CLevel(level, count) => panel_line_comment_level(fold(Fold::CLevel(*level)), right_fill, *level, *count),
        PRow::File(i, staged) => panel_line_file(view, width, right_fill, *i, *staged),
        PRow::CommentFile(path, count) => panel_line_comment_file(width, right_fill, path, *count),
        PRow::CState(resolved, count) => {
            panel_line_comment_state(fold(Fold::CState(*resolved)), right_fill, *resolved, *count)
        }
        // open: ✎ line, text, author right; resolved: ✓, muted, who resolved it
        PRow::Comment(ci) => panel_line_comment(view, width, right_fill, *ci),
        PRow::Stash(si) => panel_line_stash(view, width, rect, right_fill, *si),
        PRow::Empty(hint) => Line::styled(
            crate::ui::thread_card::fit(&format!("     {hint}"), width),
            theme::muted().add_modifier(Modifier::ITALIC),
        ),
        PRow::Elsewhere(root, count) => panel_line_elsewhere(width, right_fill, root, *count),
        PRow::Push(pushed, count) => panel_line_push(
            view,
            width,
            rect,
            fold(Fold::Push(*pushed)),
            right_fill,
            *pushed,
            *count,
        ),
        // a running push: the wave (as an agent's at work) at the panel's right edge
        PRow::Pushing(count) => panel_line_pushing(view, right_fill, *count),
        PRow::PushingCommit(ci) => panel_line_pushing_commit(view, width, right_fill, *ci),
        PRow::All => panel_line_all(view, width, right_fill),
        PRow::Commit(ci) => panel_line_commit(view, width, right_fill, *ci),
        // a file of the shown commit: the review's file in accent
        PRow::CommitFile(_, fi) => panel_line_commit_file(view, width, right_fill, *fi),
        PRow::Pick => panel_line_pick(view, width, right_fill),
    }
}

#[path = "panel_line_comments.rs"]
mod comments;
use comments::{
    panel_line_comment, panel_line_comment_file, panel_line_comment_level, panel_line_comment_state,
    panel_line_elsewhere,
};
