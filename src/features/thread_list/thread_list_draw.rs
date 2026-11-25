use super::{
    Act, Block, BorderType, Borders, Clear, Focus, Form, Frame, Line, Mode, Modifier, Paragraph, Path, ProjTab, Rect,
    RepoTab, Span, StatusTab, Style, ThreadList, View, repo_name, theme,
};

impl ThreadList {
    /// Filter, repo tabs, view tabs, the list (the host's views: `list`, the host draws them).
    pub(crate) fn draw(&mut self, frame: &mut Frame, area: Rect, tick: usize) {
        draw_thread_list(self, frame, area, tick);
    }
}

impl Form {
    pub(super) fn draw(&mut self, frame: &mut Frame, area: Rect) {
        draw_form(self, frame, area);
    }

    fn draw_project_row(
        &self,
        project_row_frame: ProjectRowFrame<'_, '_, '_>,
        line: impl Fn(&mut Frame<'_>, u16, Line<'_>),
        check: impl Fn(bool, bool, String) -> Vec<Span<'static>>,
        i: usize,
        slug: &str,
        on: bool,
    ) {
        let ProjectRowFrame { frame, y } = project_row_frame;

        let mut spans = vec![Span::raw("  ")];

        spans.extend(check(on, self.row == 2 + i, slug.to_owned()));

        if i == 0 && self.projs.len() == 1 {
            spans.push(Span::styled(
                "  0 projects in this repo: projects tab (F3)",
                theme::muted(),
            ));
        }

        line(frame, *y, Line::from(spans));
        *y += 1;
    }

    fn draw_agent_rows(
        &mut self,
        frame: &mut Frame<'_>,
        mut y: u16,
        line: impl Fn(&mut Frame<'_>, u16, Line<'_>),
        check: impl Fn(bool, bool, String) -> Vec<Span<'static>>,
    ) {
        y += 1;
        let lead = self.agents.iter().position(|agent| agent.1);

        for (i, (name, on)) in self.agents.iter().enumerate() {
            self.draw_agent_row(AgentRowLayout { frame, y: &mut y, lead }, &line, &check, i, name, *on);
        }

        if let Some(error) = &self.error {
            line(frame, y, Line::styled(error.clone(), theme::fg(theme::pal().error())));
            y += 1;
        }

        y += 1;
        line(
            frame,
            y,
            Line::styled(
                "ctrl+s: save · esc: cancel · live agents leave / join at once",
                theme::muted(),
            ),
        );
    }

    fn draw_agent_row(
        &self,
        context: AgentRowLayout<'_, '_, '_>,
        line: &impl Fn(&mut Frame<'_>, u16, Line<'_>),
        check: &impl Fn(bool, bool, String) -> Vec<Span<'static>>,
        i: usize,
        name: &str,
        on: bool,
    ) {
        let AgentRowLayout { frame, y, lead } = context;

        let mut spans = vec![Span::raw("  ")];

        spans.extend(check(
            on,
            self.row == 2 + self.projs.len() + self.links.len() + i,
            name.to_owned(),
        ));

        if Some(i) == lead {
            spans.push(Span::styled("  lead", theme::muted()));
        }

        line(frame, *y, Line::from(spans));
        *y += 1;
    }
}

fn draw_form_line(inner: Rect, frame: &mut Frame<'_>, y: u16, line: Line<'_>) {
    if y < inner.bottom() {
        frame.render_widget(Paragraph::new(line), Rect { y, height: 1, ..inner });
    }
}

fn field_label(on: bool, name: &str) -> Span<'static> {
    Span::styled(
        format!("{name:<8}"),
        if on { theme::accent_bold() } else { theme::muted() },
    )
}

fn check_spans(on: bool, hl: bool, text: String) -> Vec<Span<'static>> {
    let style = if hl { theme::pill() } else { theme::text() };

    vec![
        Span::styled(
            if on { "◼ " } else { "◻ " },
            if on { theme::accent() } else { theme::line() },
        ),
        Span::styled(text, style),
    ]
}

fn draw_mode_switch(frame: &mut Frame<'_>, filter_row: Rect, input: Rect, thread_list: &mut ThreadList) {
    frame.render_widget(
        Paragraph::new(Span::styled("› ", theme::accent())),
        Rect { width: 2, ..filter_row },
    );
    thread_list.filter.render(frame, input, true);
}

fn draw_mode_manage(frame: &mut Frame<'_>, filter_row: Rect, count_width: u16, thread_list: &mut ThreadList) {
    let label = Style::default().fg(if thread_list.filtering {
        theme::pal().accent
    } else {
        theme::pal().subtext0
    });

    frame.render_widget(Paragraph::new(Span::styled("filter › ", label)), filter_row);
    let input = Rect {
        x: filter_row.x + 9,
        width: filter_row.width.saturating_sub(9 + count_width),
        ..filter_row
    };

    if thread_list.filtering || !thread_list.filter.text().is_empty() {
        thread_list.filter.render(frame, input, thread_list.filtering);
    } else {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "ctrl+f: fuzzy search thread names",
                theme::fg(theme::pal().overlay0),
            )),
            input,
        );
    }
}

fn draw_status_tabs(frame: &mut Frame<'_>, status_row: Rect, thread_list: &mut ThreadList) {
    let tab = thread_list.stat_tab();

    thread_list.stats.i = StatusTab::ALL.iter().position(|status| *status == tab).unwrap_or(0);
    let counts = thread_list.stat_counts();
    let labels: Vec<String> = StatusTab::ALL
        .iter()
        .zip(counts)
        .map(|(status, count)| format!("{} {count}", status.label()))
        .collect();

    thread_list.stats.draw(
        frame,
        status_row,
        "status ",
        &labels,
        thread_list.focus == Focus::Status,
    );

    if let Some(x) = thread_list.stats.end().filter(|_| !tab.hint().is_empty()) {
        let hint_area = Rect {
            x: x + 1,
            width: status_row.right().saturating_sub(x + 1),
            ..status_row
        };

        frame.render_widget(Paragraph::new(Span::styled(tab.hint(), theme::muted())), hint_area);
    }
}

fn draw_thread_list(state: &mut ThreadList, frame: &mut Frame, area: Rect, tick: usize) {
    state.poll(true);
    state.hit_regions.rows.clear();
    state.hit_regions.act_rows.clear();

    if area.height < 6 {
        return;
    }

    let row = |i: u16| Rect {
        y: area.y + i,
        height: 1,
        ..area
    };

    draw_filter_header(state, frame, row(0));

    draw_tab_rows(state, frame, row);

    // status tabs (active) / the archived note: a fixed row, out of the list's scroll
    let status_row = row(4);

    if state.has_stats() {
        draw_status_tabs(frame, status_row, state);
    } else if state.view == View::Archived {
        let count = state.visible().len();

        frame.render_widget(
            Paragraph::new(Span::styled(
                format!("archived · {count} · branches kept, worktrees gone"),
                theme::muted().add_modifier(Modifier::BOLD),
            )),
            status_row,
        );
    }

    let mut list = Rect {
        y: area.y + 5,
        height: area.height.saturating_sub(5),
        ..area
    };

    // the question / the status: the last row
    let foot = Rect {
        y: list.bottom().saturating_sub(1),
        height: 1,
        ..list
    };

    let foot_line = footer_line(state);

    if foot_line.is_some() {
        list.height = list.height.saturating_sub(2);
    }

    state.list = list;

    if state.view.cards() {
        state.draw_cards(frame, list, tick);
    }

    if let Some(line) = foot_line {
        frame.render_widget(Clear, foot);
        frame.render_widget(Paragraph::new(line), foot);
    }

    state.draw_actions(frame, list);

    if let Some(form) = state.form.as_mut() {
        form.draw(frame, area);
    }
}

fn status_line(state: &ThreadList) -> Option<Line<'static>> {
    state.job_state.status.as_ref().map(|(text, err, _)| {
        Line::styled(
            format!(" {text}"),
            if *err {
                theme::fg(theme::pal().error())
            } else {
                theme::fg(theme::pal().success())
            },
        )
    })
}

struct AgentRowLayout<'frame, 'buffer, 'row> {
    frame: &'frame mut Frame<'buffer>,
    y: &'row mut u16,
    lead: Option<usize>,
}

fn draw_form(state: &mut Form, frame: &mut Frame, area: Rect) {
    let bounds = form_bounds(state, area);

    let inner = draw_form_frame(frame, bounds);
    let mut y = inner.y;

    let line = |frame: &mut Frame, y: u16, text: Line| {
        draw_form_line(inner, frame, y, text);
    };

    let label = |on: bool, name: &str| field_label(on, name);

    line(frame, y, Line::from(label(state.row == 0, "title")));

    if y < inner.bottom() {
        state.title.render(
            frame,
            Rect {
                x: inner.x + 8,
                y,
                width: inner.width.saturating_sub(8),
                height: 1,
            },
            state.row == 0,
        );
    }

    y += 1;

    let repo = match state.repos.get(state.repo_i).cloned().flatten() {
        Some(path) => format!("{} · {}", repo_name(Path::new(&path)), path),
        None => format!("auto: {} (the repo of its dir)", state.auto),
    };

    let style = if state.row == 1 { theme::pill() } else { theme::text() };

    line(
        frame,
        y,
        Line::from(vec![
            label(state.row == 1, "repo"),
            Span::styled(format!("‹ {repo} ›"), style),
            Span::styled("  ←→", theme::muted()),
        ]),
    );
    y += 1;

    let check = |on: bool, hl: bool, text: String| check_spans(on, hl, text);

    line(
        frame,
        y,
        Line::styled(
            "projects (space: on / off; auto: the agents pick)",
            theme::muted().add_modifier(Modifier::BOLD),
        ),
    );
    draw_form_rows(state, frame, y, line, check);
}

fn draw_form_rows(
    state: &mut Form,
    frame: &mut Frame<'_>,
    mut y: u16,
    line: impl Fn(&mut Frame<'_>, u16, Line<'_>),
    check: impl Fn(bool, bool, String) -> Vec<Span<'static>>,
) {
    y += 1;

    for (i, (slug, on)) in state.projs.iter().enumerate() {
        state.draw_project_row(ProjectRowFrame { frame, y: &mut y }, &line, &check, i, slug, *on);
    }

    line(
        frame,
        y,
        Line::styled(
            "linked repos (space: keep / drop)",
            theme::muted().add_modifier(Modifier::BOLD),
        ),
    );
    y += 1;

    if state.links.is_empty() {
        line(frame, y, Line::styled("  0 linked repos", theme::muted()));
        y += 1;
    }

    for (i, (dir, on)) in state.links.iter().enumerate() {
        let mut spans = vec![Span::raw("  ")];

        spans.extend(check(*on, state.row == 2 + state.projs.len() + i, dir.clone()));
        line(frame, y, Line::from(spans));
        y += 1;
    }

    line(
        frame,
        y,
        Line::styled(
            "agents (space: on / off; the 1st on: lead)",
            theme::muted().add_modifier(Modifier::BOLD),
        ),
    );
    state.draw_agent_rows(frame, y, line, check);
}

fn draw_filter_header(state: &mut ThreadList, frame: &mut Frame, filter_row: Rect) {
    // the filter
    let total = state
        .cards
        .iter()
        .filter(|card| card.archived() == (state.view == View::Archived))
        .count();
    let shown = state.visible().len();

    let count_label = if state.view.cards() && shown != total {
        format!("{shown} of {total} ")
    } else {
        String::new()
    };

    let count_width = unicode_width::UnicodeWidthStr::width(count_label.as_str()) as u16;
    let input = Rect {
        x: filter_row.x + 2,
        width: filter_row.width.saturating_sub(2 + count_width),
        ..filter_row
    };

    match state.mode {
        Mode::Switch => {
            draw_mode_switch(frame, filter_row, input, state);
        }
        Mode::Manage => {
            draw_mode_manage(frame, filter_row, count_width, state);
        }
    }

    if count_width > 0 {
        frame.render_widget(
            Paragraph::new(Line::styled(count_label, theme::fg(theme::pal().subtext0)).right_aligned()),
            filter_row,
        );
    }
}

fn draw_tab_rows(state: &mut ThreadList, frame: &mut Frame, row: impl Fn(u16) -> Rect) {
    // repo tabs, view tabs
    let tabs = state.repo_tabs();
    let cur = state.repo_tab();

    state.repos.i = tabs.iter().position(|tab| *tab == cur).unwrap_or(0);
    let labels: Vec<String> = tabs.iter().map(RepoTab::label).collect();

    state
        .repos
        .draw(frame, row(1), "repo   ", &labels, state.focus == Focus::Repos);
    let ptabs = state.proj_tabs();
    let pcur = state.proj_tab();

    state.projs.i = ptabs.iter().position(|tab| *tab == pcur).unwrap_or(0);
    let labels: Vec<String> = ptabs.iter().map(ProjTab::label).collect();

    state
        .projs
        .draw(frame, row(2), "project", &labels, state.focus == Focus::Projects);
    let all = state.views_all();

    state.views.i = all.iter().position(|view| *view == state.view).unwrap_or(0);
    let labels: Vec<String> = all.iter().map(|view| state.view_label(*view)).collect();

    state
        .views
        .draw(frame, row(3), "show   ", &labels, state.focus == Focus::Views);
}

fn footer_line(state: &ThreadList) -> Option<Line<'static>> {
    if let Some(pending) = &state.job_state.pending {
        let count = pending.ids.len();

        // threads with work not in the base: the question names it
        let open: Vec<String> = pending
            .ids
            .iter()
            .filter_map(|id| {
                state
                    .cards
                    .iter()
                    .find(|card| &card.id == id)?
                    .land
                    .clone()
                    .filter(crate::domain::landed::Land::open)
            })
            .map(|land| land.badge())
            .collect();

        let ask_archive = match open.as_slice() {
            [] => format!(" archive {count} thread(s)? "),
            [one] if count == 1 => format!(" {one}: archive anyway? "),
            _ => format!(" archive {count} thread(s), {} open (work not landed)? ", open.len()),
        };

        let (ask, why) = match pending.act {
            Act::Archive => (
                ask_archive,
                "hosts stop, worktrees go (uncommitted files → git stash), branches stay",
            ),
            _ => (
                format!(" delete {count} thread(s) + worktree(s)? "),
                "uncommitted files → git stash, pushed branches go",
            ),
        };

        Some(Line::from(vec![
            Span::styled(ask, theme::pill().add_modifier(Modifier::BOLD)),
            Span::styled(" enter: yes · esc: no  ", theme::accent()),
            Span::styled(why, theme::muted()),
        ]))
    } else {
        status_line(state)
    }
}

fn form_bounds(state: &Form, area: Rect) -> Rect {
    let width = area.width.saturating_sub(4).min(76);
    let body = 2
        + 1
        + state.projs.len()
        + 1
        + state.links.len().max(1)
        + 1
        + state.agents.len()
        + 2
        + usize::from(state.error.is_some());
    let height = (body as u16 + 2).min(area.height);

    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + 1.min(area.height.saturating_sub(height)),
        width,
        height,
    }
}

fn draw_form_frame(frame: &mut Frame, area: Rect) -> Rect {
    frame.render_widget(Clear, area);
    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(theme::accent())
        .title(Span::styled(" edit thread ", theme::accent_bold()))
        .style(theme::text());
    let inner = block.inner(area);

    frame.render_widget(block, area);

    Rect {
        x: inner.x + 1,
        width: inner.width.saturating_sub(2),
        ..inner
    }
}

struct ProjectRowFrame<'frame, 'buffer, 'row> {
    frame: &'frame mut Frame<'buffer>,
    y: &'row mut u16,
}
