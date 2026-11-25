use std::fmt::Write as FormatWrite;

use super::{
    ARM, Edit, Kind, Line, Merge, MergeBtn, Modifier, Pr, Ready, Row, Span, State, Stop, Style, Tab, TextArea,
    UnicodeWidthStr, Verdict, busy_label, check_name, chip, gh, hex, secs, theme,
};

struct OverviewRows<'context> {
    pr: &'context Pr,
    width: usize,
    focus: Option<Stop>,
    edit: Option<&'context (Edit, TextArea)>,
    tab: Option<&'context Tab>,
    rows: Vec<Row>,
}

impl Merge {
    /// The overview's rows of P in W columns.
    pub(super) fn rows(&self, path_value: &Pr, width_value: usize, focused: bool) -> Vec<Row> {
        let tab = self.tab_ref();
        let focus = if focused { tab.and_then(|tab| tab.focus) } else { None };
        let edit = tab.and_then(|tab| tab.input.edit.as_ref());
        let mut overview = OverviewRows {
            pr: path_value,
            width: width_value,
            focus,
            edit,
            tab,
            rows: Vec::new(),
        };

        append_title(&mut overview);
        append_status(&mut overview);
        append_branches(&mut overview);
        append_labels(&mut overview);
        actions::append_actions(&mut overview);
        checks::append_checks(&mut overview);
        conversation::append_comments(&mut overview);
        conversation::append_description(&mut overview);

        overview.rows
    }
}
fn highlight(focus: Option<Stop>, stop: Stop, style: Style) -> Style {
    if focus == Some(stop) {
        style
            .bg(theme::pal().accent)
            .fg(theme::pal().on_accent)
            .add_modifier(Modifier::BOLD)
    } else {
        style
    }
}
fn input_rows(width: usize, input: &TextArea, single: bool) -> u16 {
    if single {
        3
    } else {
        (input.rows_for(width.saturating_sub(2) as u16) as u16).clamp(3, 14) + 2
    }
}
#[path = "merge_overview_checks.rs"]
mod checks;
#[path = "merge_overview_conversation.rs"]
mod conversation;

fn append_title(overview: &mut OverviewRows<'_>) {
    // hero: the title
    if let Some((Edit::Title, ta)) = overview.edit {
        overview.rows.push(Row {
            line: Line::raw(""),
            stops: Vec::new(),
            input: input_rows(overview.width, ta, true),
        });
    } else {
        let title = format!("{} #{}", overview.pr.title, overview.pr.pr.number);
        let st = highlight(
            overview.focus,
            Stop::Title,
            Style::default()
                .fg(theme::pal().text)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        );

        overview.rows.push(Row::stop(
            Line::from(Span::styled(crate::ui::thread_card::fit(&title, overview.width), st)),
            Stop::Title,
        ));
    }
}

fn append_status(overview: &mut OverviewRows<'_>) {
    // the status (a dropdown), the author, the times
    let (sc, sl) = match overview.pr.state {
        State::Open => (theme::pal().success(), "● open"),
        State::Draft => (theme::pal().subtext0, "◌ draft"),
        State::Merged => (theme::pal().merged(), "◆ merged"),
        State::Closed => (theme::pal().error(), "✕ closed"),
    };

    let status = format!(" {sl} ▾ ");
    let sw = status.width() as u16;
    let st = chip(sc, overview.focus == Some(Stop::Status));
    let mut meta = format!("  {} · opened {}", overview.pr.author, gh::stamp(overview.pr.created));

    if let Some(message) = overview.pr.merged {
        write!(meta, " · merged {}", gh::stamp(message)).unwrap();
    } else if let (Some(character), State::Closed) = (overview.pr.closed, overview.pr.state) {
        write!(meta, " · closed {}", gh::stamp(character)).unwrap();
    }

    overview.rows.push(Row {
        line: Line::from(vec![Span::styled(status, st), Span::styled(meta, theme::muted())]),
        stops: vec![(0, sw, Stop::Status)],
        input: 0,
    });
}

fn append_branches(overview: &mut OverviewRows<'_>) {
    // the branches, the size
    overview.rows.push(Row::new(Line::from(vec![
        Span::styled(overview.pr.base.clone(), theme::fg(theme::pal().info())),
        Span::styled(" ← ", theme::muted()),
        Span::styled(overview.pr.head.clone(), theme::fg(theme::pal().info())),
        Span::styled("  ", theme::muted()),
        Span::styled(
            format!("+{}", overview.pr.additions),
            Style::default().fg(theme::pal().success()),
        ),
        Span::raw(" "),
        Span::styled(
            format!("−{}", overview.pr.deletions),
            Style::default().fg(theme::pal().error()),
        ),
        Span::styled(
            format!(" · {} files · {} commits", overview.pr.files, overview.pr.commits.len()),
            theme::muted(),
        ),
    ])));
}

fn append_labels(overview: &mut OverviewRows<'_>) {
    // the labels: × removes, + adds
    let mut spans = Vec::new();
    let mut stops = Vec::new();
    let mut x = 0u16;

    for (i, line_value) in overview.pr.labels.iter().enumerate() {
        let text = if overview.pr.can_edit {
            format!(" {} × ", line_value.name)
        } else {
            format!(" {} ", line_value.name)
        };

        let tw = text.width() as u16;

        spans.push(Span::styled(
            text,
            chip(hex(&line_value.color), overview.focus == Some(Stop::Label(i))),
        ));
        spans.push(Span::raw(" "));
        stops.push((x, tw, Stop::Label(i)));
        x += tw + 1;
    }

    if overview.pr.can_edit {
        spans.push(Span::styled(
            " + label ",
            chip(theme::pal().accent, overview.focus == Some(Stop::AddLabel)),
        ));
        stops.push((x, 9, Stop::AddLabel));
    } else if overview.pr.labels.is_empty() {
        spans.push(Span::styled("0 labels", theme::muted()));
    }

    overview.rows.push(Row {
        line: Line::from(spans),
        stops,
        input: 0,
    });
    overview.rows.push(Row::new(Line::raw("")));
}

#[path = "merge_overview_actions.rs"]
mod actions;
