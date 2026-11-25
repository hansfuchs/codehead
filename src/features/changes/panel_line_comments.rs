use super::{Level, Line, Modifier, Span, Style, UnicodeWidthStr, View, level_color, split_path, theme};

pub(super) fn panel_line_elsewhere(
    width_value: usize,
    right_fill: impl Fn(&[Span<'static>], &[Span<'static>]) -> Line<'static>,
    root: &std::path::Path,
    count_4: usize,
) -> Line<'static> {
    let name = root.file_name().map_or_else(
        || root.to_string_lossy().into_owned(),
        |count_5| count_5.to_string_lossy().into_owned(),
    );
    let text = format!("   ↗ {count_4} open in {name}");

    right_fill(
        &[Span::styled(
            crate::ui::thread_card::fit(&text, width_value.saturating_sub(8)),
            theme::muted(),
        )],
        &[Span::styled("enter ", theme::muted())],
    )
}

pub(super) fn panel_line_comment_state(
    indicator: &'static str,
    right_fill: impl Fn(&[Span<'static>], &[Span<'static>]) -> Line<'static>,
    resolved: bool,
    count_3: usize,
) -> Line<'static> {
    let (label, color) = if resolved {
        ("RESOLVED", theme::pal().subtext0)
    } else {
        ("OPEN", theme::pal().progress())
    };

    let st = Style::default().fg(color).add_modifier(Modifier::BOLD);

    right_fill(
        &[Span::styled(format!("   {indicator} {label}"), st)],
        &[Span::styled(format!("{count_3} "), theme::muted())],
    )
}

pub(super) fn panel_line_comment_file(
    width_value: usize,
    right_fill: impl Fn(&[Span<'static>], &[Span<'static>]) -> Line<'static>,
    path: &str,
    comment: usize,
) -> Line<'static> {
    let (name, dir) = split_path(path);
    let room = width_value.saturating_sub(8);
    let name = crate::ui::thread_card::fit(name, room);
    let dir_room = room.saturating_sub(name.width() + 1);
    let mut left = vec![Span::raw("   "), Span::styled(name, theme::text())];

    if dir_room > 3 && !dir.is_empty() {
        left.push(Span::styled(
            format!(" {}", crate::ui::thread_card::fit(dir, dir_room)),
            theme::muted(),
        ));
    }

    right_fill(&left, &[Span::styled(format!("{comment} "), theme::muted())])
}

pub(super) fn panel_line_comment_level(
    indicator: &'static str,
    right_fill: impl Fn(&[Span<'static>], &[Span<'static>]) -> Line<'static>,
    line_value: Level,
    count_2: usize,
) -> Line<'static> {
    let st = Style::default()
        .fg(level_color(line_value))
        .add_modifier(Modifier::BOLD);

    right_fill(
        &[Span::styled(format!("   {} {}", indicator, line_value.label()), st)],
        &[Span::styled(format!("{count_2} "), theme::muted())],
    )
}

pub(super) fn panel_line_comment(
    view: &mut View,
    width_value: usize,
    right_fill: impl Fn(&[Span<'static>], &[Span<'static>]) -> Line<'static>,
    ci: usize,
) -> Line<'static> {
    let Some(comment_value) = view.comment_state.comments.get(ci) else {
        return Line::raw("");
    };

    let (mark, st) = if comment_value.open() {
        ("✎", theme::fg(theme::pal().progress()))
    } else {
        ("✓", theme::muted())
    };

    let who = if comment_value.open() || comment_value.resolved_by.is_empty() {
        comment_value.author()
    } else {
        format!("{} ✓ @{}", comment_value.author(), comment_value.resolved_by)
    };

    // replies: ↳ count; unseen agent replies: ● count (accent)
    let new = crate::features::changes::threads::unseen(comment_value);

    let replies = if comment_value.replies.is_empty() {
        String::new()
    } else {
        format!("↳{} ", comment_value.replies.len())
    };

    let fresh = if new > 0 { format!("●{new} ") } else { String::new() };
    let right = vec![
        Span::styled(fresh.clone(), theme::accent_bold()),
        Span::styled(replies.clone(), theme::muted()),
        Span::styled(format!("{who} "), theme::muted()),
    ];
    let head = format!("     {mark} {}: ", comment_value.at());
    let text = comment_value.text.lines().next().unwrap_or_default();
    let room = width_value.saturating_sub(head.width() + who.width() + replies.width() + fresh.width() + 2);

    right_fill(
        &[
            Span::styled(head, st.add_modifier(Modifier::BOLD)),
            Span::styled(crate::ui::thread_card::fit(text, room), st),
        ],
        &right,
    )
}
