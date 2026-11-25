use super::{Frame, Line, Merge, Modifier, Paragraph, PrRef, Rect, SLEEP, Span, Style, Sub, UnicodeWidthStr, theme};

pub(super) fn draw_pull_request_tabs(merge: &mut Merge, frame: &mut Frame<'_>, area: Rect, tabs_focused: bool) {
    let mut x = area.x + 1;
    let mut spans = Vec::new();

    for (i, pr) in merge.prs.iter().enumerate() {
        draw_pull_request_tab(
            &mut merge.layout.tab_hits,
            TabLabel {
                area,
                current: merge.cur,
                on: tabs_focused,
            },
            &mut x,
            &mut spans,
            i,
            pr,
        );
    }

    let hint = if merge.prs.len() > 1 { "  [ ]: PR" } else { "" };

    spans.push(Span::styled(hint, theme::muted()));
    frame.render_widget(Paragraph::new(Line::from(spans)), Rect { height: 1, ..area });
}

pub(super) fn draw_sub_tabs(merge: &mut Merge, frame: &mut Frame<'_>, area: Rect, subs_focused: bool, sub: Sub) {
    let mut x = area.x + 1;
    let mut spans = Vec::new();

    for (changes, label) in [(Sub::Overview, " overview "), (Sub::Changes, " changes ")] {
        draw_sub_tab(
            merge,
            SubTabLabel {
                area,
                on: subs_focused,
                sub,
            },
            &mut x,
            &mut spans,
            changes,
            label,
        );
    }

    let busy = merge
        .tab_ref()
        .map(|thread_2| {
            (
                thread_2.reads.reading,
                thread_2.input.busy.is_some(),
                thread_2.reads.read_at,
            )
        })
        .unwrap_or_default();

    let state = if busy.1 {
        "  · gh …".to_string()
    } else if merge.active.elapsed() > SLEEP {
        "  · asleep: a key wakes it".to_string()
    } else if busy.0 {
        "  · reading…".to_string()
    } else {
        busy.2
            .map(|thread_3| format!("  · read {}s ago · r: again", thread_3.elapsed().as_secs()))
            .unwrap_or_default()
    };

    spans.push(Span::styled(state, theme::muted()));
    spans.push(Span::styled("  < >: view", theme::muted()));
    frame.render_widget(
        Paragraph::new(Line::from(spans)),
        Rect {
            y: area.y + 1,
            height: 1,
            ..area
        },
    );
}

pub(super) fn draw_sub_tab(
    merge: &mut Merge,
    sub_tab_label: SubTabLabel,
    x: &mut u16,
    spans: &mut Vec<Span<'_>>,
    view: Sub,
    label: &'static str,
) {
    let SubTabLabel { area, on, sub } = sub_tab_label;

    let label_width = label.width() as u16;

    let st = if view == sub {
        if on {
            theme::pill().add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(theme::pal().accent)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        }
    } else {
        theme::muted()
    };

    merge.layout.sub_hits.push((
        Rect {
            x: *x,
            y: area.y + 1,
            width: label_width,
            height: 1,
        },
        view,
    ));
    spans.push(Span::styled(label, st));
    *x += label_width + 1;
}

pub(super) fn draw_pull_request_tab(
    tab_hits: &mut Vec<(Rect, usize)>,
    look: TabLabel,
    x: &mut u16,
    spans: &mut Vec<Span<'_>>,
    i: usize,
    pr: &PrRef,
) {
    let TabLabel { area, current, on } = look;
    let label = format!(" {} ", pr.label());
    let width_value = label.width() as u16;

    let st = if i == current {
        if on {
            theme::pill().add_modifier(Modifier::BOLD)
        } else {
            theme::pill()
        }
    } else {
        theme::muted()
    };

    tab_hits.push((
        Rect {
            x: *x,
            y: area.y,
            width: width_value,
            height: 1,
        },
        i,
    ));
    spans.push(Span::styled(label, st));
    spans.push(Span::raw(" "));
    *x += width_value + 1;
}

#[derive(Clone, Copy)]
pub(super) struct TabLabel {
    area: Rect,
    current: usize,
    on: bool,
}

#[derive(Clone, Copy)]
pub(super) struct SubTabLabel {
    area: Rect,
    on: bool,
    sub: Sub,
}
