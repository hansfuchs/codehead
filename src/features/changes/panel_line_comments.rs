use super::{Level, Line, Modifier, Span, Style, UnicodeWidthStr, View, level_color, split_path, theme};

pub(super) fn panel_line_elsewhere(
    width: usize,
    right_fill: impl Fn(&[Span<'static>], &[Span<'static>]) -> Line<'static>,
    root: &std::path::Path,
    count: usize,
) -> Line<'static> {
    let name = root.file_name().map_or_else(
        || root.to_string_lossy().into_owned(),
        |name| name.to_string_lossy().into_owned(),
    );
    let text = format!("   ↗ {count} open in {name}");

    right_fill(
        &[Span::styled(
            crate::ui::thread_card::fit(&text, width.saturating_sub(8)),
            theme::muted(),
        )],
        &[Span::styled("enter ", theme::muted())],
    )
}

pub(super) fn panel_line_comment_state(
    indicator: &'static str,
    right_fill: impl Fn(&[Span<'static>], &[Span<'static>]) -> Line<'static>,
    resolved: bool,
    count: usize,
) -> Line<'static> {
    let (label, color) = if resolved {
        ("RESOLVED", theme::pal().subtext0)
    } else {
        ("OPEN", theme::pal().progress())
    };

    let st = Style::default().fg(color).add_modifier(Modifier::BOLD);

    right_fill(
        &[Span::styled(format!("   {indicator} {label}"), st)],
        &[Span::styled(format!("{count} "), theme::muted())],
    )
}

pub(super) fn panel_line_comment_file(
    width: usize,
    right_fill: impl Fn(&[Span<'static>], &[Span<'static>]) -> Line<'static>,
    path: &str,
    count: usize,
) -> Line<'static> {
    let (name, dir) = split_path(path);
    let room = width.saturating_sub(8);
    let name = crate::ui::thread_card::fit(name, room);
    let dir_room = room.saturating_sub(name.width() + 1);
    let mut left = vec![Span::raw("   "), Span::styled(name, theme::text())];

    if dir_room > 3 && !dir.is_empty() {
        left.push(Span::styled(
            format!(" {}", crate::ui::thread_card::fit(dir, dir_room)),
            theme::muted(),
        ));
    }

    right_fill(&left, &[Span::styled(format!("{count} "), theme::muted())])
}

pub(super) fn panel_line_comment_level(
    indicator: &'static str,
    right_fill: impl Fn(&[Span<'static>], &[Span<'static>]) -> Line<'static>,
    level: Level,
    count: usize,
) -> Line<'static> {
    let st = Style::default().fg(level_color(level)).add_modifier(Modifier::BOLD);

    right_fill(
        &[Span::styled(format!("   {} {}", indicator, level.label()), st)],
        &[Span::styled(format!("{count} "), theme::muted())],
    )
}

pub(super) fn panel_line_comment(
    view: &mut View,
    width: usize,
    right_fill: impl Fn(&[Span<'static>], &[Span<'static>]) -> Line<'static>,
    ci: usize,
) -> Line<'static> {
    let Some(comment) = view.comment_state.comments.get(ci) else {
        return Line::raw("");
    };

    let (mark, st) = if comment.open() {
        ("✎", theme::fg(theme::pal().progress()))
    } else {
        ("✓", theme::muted())
    };

    let who = if comment.open() || comment.resolved_by.is_empty() {
        comment.author()
    } else {
        format!("{} ✓ @{}", comment.author(), comment.resolved_by)
    };

    // replies: ↳ count; unseen agent replies: ● count (accent)
    let new = crate::features::changes::threads::unseen(comment);

    let replies = if comment.replies.is_empty() {
        String::new()
    } else {
        format!("↳{} ", comment.replies.len())
    };

    let fresh = if new > 0 { format!("●{new} ") } else { String::new() };
    let right = vec![
        Span::styled(fresh.clone(), theme::accent_bold()),
        Span::styled(replies.clone(), theme::muted()),
        Span::styled(format!("{who} "), theme::muted()),
    ];
    let head = format!("     {mark} {}: ", comment.at());
    let text = comment.text.lines().next().unwrap_or_default();
    let room = width.saturating_sub(head.width() + who.width() + replies.width() + fresh.width() + 2);

    right_fill(
        &[
            Span::styled(head, st.add_modifier(Modifier::BOLD)),
            Span::styled(crate::ui::thread_card::fit(text, room), st),
        ],
        &right,
    )
}
