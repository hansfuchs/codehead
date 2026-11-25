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

    fn draw_text_value(
        &self,
        project_row_frame: ProjectRowFrame<'_, '_, '_>,
        line: impl Fn(&mut Frame<'_>, u16, Line<'_>),
        check: impl Fn(bool, bool, String) -> Vec<Span<'static>>,
        i: usize,
        slug: &str,
        on: bool,
    ) {
        let ProjectRowFrame { frame, y } = project_row_frame;

        let mut text_value = vec![Span::raw("  ")];

        text_value.extend(check(on, self.row == 2 + i, slug.to_owned()));

        if i == 0 && self.projs.len() == 1 {
            text_value.push(Span::styled(
                "  0 projects in this repo: projects tab (F3)",
                theme::muted(),
            ));
        }

        line(frame, *y, Line::from(text_value));
        *y += 1;
    }

    fn finish_draw(
        &mut self,
        frame: &mut Frame<'_>,
        mut y: u16,
        line: impl Fn(&mut Frame<'_>, u16, Line<'_>),
        check: impl Fn(bool, bool, String) -> Vec<Span<'static>>,
    ) {
        y += 1;
        let lead = self.agents.iter().position(|agent| agent.1);

        for (i, (name, on)) in self.agents.iter().enumerate() {
            self.finish_draw_text_(AgentRowLayout { frame, y: &mut y, lead }, &line, &check, i, name, *on);
        }

        if let Some(error_value) = &self.error {
            line(
                frame,
                y,
                Line::styled(error_value.clone(), theme::fg(theme::pal().error())),
            );
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

    fn finish_draw_text_(
        &self,
        context: AgentRowLayout<'_, '_, '_>,
        line: &impl Fn(&mut Frame<'_>, u16, Line<'_>),
        check: &impl Fn(bool, bool, String) -> Vec<Span<'static>>,
        i: usize,
        name: &str,
        on: bool,
    ) {
        let AgentRowLayout { frame, y, lead } = context;

        let mut text_3 = vec![Span::raw("  ")];

        text_3.extend(check(
            on,
            self.row == 2 + self.projs.len() + self.links.len() + i,
            name.to_owned(),
        ));

        if Some(i) == lead {
            text_3.push(Span::styled("  lead", theme::muted()));
        }

        line(frame, *y, Line::from(text_3));
        *y += 1;
    }
}

fn draw_branch(inner: Rect, frame_value: &mut Frame<'_>, y: u16, line_value: Line<'_>) {
    if y < inner.bottom() {
        frame_value.render_widget(Paragraph::new(line_value), Rect { y, height: 1, ..inner });
    }
}

fn draw_body_state(on: bool, thread: &str) -> Span<'static> {
    Span::styled(
        format!("{thread:<8}"),
        if on { theme::accent_bold() } else { theme::muted() },
    )
}

fn draw_body(on: bool, hl: bool, text: String) -> Vec<Span<'static>> {
    let st = if hl { theme::pill() } else { theme::text() };

    vec![
        Span::styled(
            if on { "◼ " } else { "◻ " },
            if on { theme::accent() } else { theme::line() },
        ),
        Span::styled(text, st),
    ]
}

fn draw_mode_switch(frame: &mut Frame<'_>, fr: Rect, input: Rect, view_state: &mut ThreadList) {
    frame.render_widget(
        Paragraph::new(Span::styled("› ", theme::accent())),
        Rect { width: 2, ..fr },
    );
    view_state.filter.render(frame, input, true);
}

fn draw_mode_manage(frame: &mut Frame<'_>, fr: Rect, cw: u16, view_state: &mut ThreadList) {
    let label = Style::default().fg(if view_state.filtering {
        theme::pal().accent
    } else {
        theme::pal().subtext0
    });

    frame.render_widget(Paragraph::new(Span::styled("filter › ", label)), fr);
    let input = Rect {
        x: fr.x + 9,
        width: fr.width.saturating_sub(9 + cw),
        ..fr
    };

    if view_state.filtering || !view_state.filter.text().is_empty() {
        view_state.filter.render(frame, input, view_state.filtering);
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

fn draw_count_value(frame: &mut Frame<'_>, sr: Rect, view_state: &mut ThreadList) {
    let st = view_state.stat_tab();

    view_state.stats.i = StatusTab::ALL.iter().position(|thread_2| *thread_2 == st).unwrap_or(0);
    let count_value = view_state.stat_counts();
    let labels: Vec<String> = StatusTab::ALL
        .iter()
        .zip(count_value)
        .map(|(thread_3, count_2)| format!("{} {count_2}", thread_3.label()))
        .collect();

    view_state
        .stats
        .draw(frame, sr, "status ", &labels, view_state.focus == Focus::Status);

    if let Some(x) = view_state.stats.end().filter(|_| !st.hint().is_empty()) {
        let area_value = Rect {
            x: x + 1,
            width: sr.right().saturating_sub(x + 1),
            ..sr
        };

        frame.render_widget(Paragraph::new(Span::styled(st.hint(), theme::muted())), area_value);
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
    let sr = row(4);

    if state.has_stats() {
        draw_count_value(frame, sr, state);
    } else if state.view == View::Archived {
        let count_3 = state.visible().len();

        frame.render_widget(
            Paragraph::new(Span::styled(
                format!("archived · {count_3} · branches kept, worktrees gone"),
                theme::muted().add_modifier(Modifier::BOLD),
            )),
            sr,
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

    if let Some(line_value) = foot_line {
        frame.render_widget(Clear, foot);
        frame.render_widget(Paragraph::new(line_value), foot);
    }

    state.draw_actions(frame, list);

    if let Some(form) = state.form.as_mut() {
        form.draw(frame, area);
    }
}

fn draw_thread_list_body(state: &ThreadList) -> Option<Line<'static>> {
    state.job_state.status.as_ref().map(|(thread_4, err, _)| {
        Line::styled(
            format!(" {thread_4}"),
            if *err {
                theme::fg(theme::pal().error())
            } else {
                theme::fg(theme::pal().success())
            },
        )
    })
}

struct AgentRowLayout<'reference_0, 'reference_1, 'reference_2> {
    frame: &'reference_0 mut Frame<'reference_1>,
    y: &'reference_2 mut u16,
    lead: Option<usize>,
}

fn draw_form(state: &mut Form, frame: &mut Frame, area: Rect) {
    let area_value = form_bounds(state, area);

    let inner = draw_form_frame(frame, area_value);
    let mut y = inner.y;

    let line = |frame_value: &mut Frame, y: u16, line_value: Line| {
        draw_branch(inner, frame_value, y, line_value);
    };

    let label = |on: bool, thread: &str| draw_body_state(on, thread);

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

    let st = if state.row == 1 { theme::pill() } else { theme::text() };

    line(
        frame,
        y,
        Line::from(vec![
            label(state.row == 1, "repo"),
            Span::styled(format!("‹ {repo} ›"), st),
            Span::styled("  ←→", theme::muted()),
        ]),
    );
    y += 1;

    let check = |on: bool, hl: bool, text: String| draw_body(on, hl, text);

    line(
        frame,
        y,
        Line::styled(
            "projects (space: on / off; auto: the agents pick)",
            theme::muted().add_modifier(Modifier::BOLD),
        ),
    );
    finish_draw_form(state, frame, y, line, check);
}

fn finish_draw_form(
    state: &mut Form,
    frame: &mut Frame<'_>,
    mut y: u16,
    line: impl Fn(&mut Frame<'_>, u16, Line<'_>),
    check: impl Fn(bool, bool, String) -> Vec<Span<'static>>,
) {
    y += 1;

    for (i, (slug, on)) in state.projs.iter().enumerate() {
        state.draw_text_value(ProjectRowFrame { frame, y: &mut y }, &line, &check, i, slug, *on);
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
        let mut text_2 = vec![Span::raw("  ")];

        text_2.extend(check(*on, state.row == 2 + state.projs.len() + i, dir.clone()));
        line(frame, y, Line::from(text_2));
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
    state.finish_draw(frame, y, line, check);
}

fn draw_filter_header(state: &mut ThreadList, frame: &mut Frame, fr: Rect) {
    // the filter
    let total = state
        .cards
        .iter()
        .filter(|character| character.archived() == (state.view == View::Archived))
        .count();
    let shown = state.visible().len();

    let count = if state.view.cards() && shown != total {
        format!("{shown} of {total} ")
    } else {
        String::new()
    };

    let cw = unicode_width::UnicodeWidthStr::width(count.as_str()) as u16;
    let input = Rect {
        x: fr.x + 2,
        width: fr.width.saturating_sub(2 + cw),
        ..fr
    };

    match state.mode {
        Mode::Switch => {
            draw_mode_switch(frame, fr, input, state);
        }
        Mode::Manage => {
            draw_mode_manage(frame, fr, cw, state);
        }
    }

    if cw > 0 {
        frame.render_widget(
            Paragraph::new(Line::styled(count, theme::fg(theme::pal().subtext0)).right_aligned()),
            fr,
        );
    }
}

fn draw_tab_rows(state: &mut ThreadList, frame: &mut Frame, row: impl Fn(u16) -> Rect) {
    // repo tabs, view tabs
    let tabs = state.repo_tabs();
    let cur = state.repo_tab();

    state.repos.i = tabs.iter().position(|thread| *thread == cur).unwrap_or(0);
    let labels: Vec<String> = tabs.iter().map(RepoTab::label).collect();

    state
        .repos
        .draw(frame, row(1), "repo   ", &labels, state.focus == Focus::Repos);
    let ptabs = state.proj_tabs();
    let pcur = state.proj_tab();

    state.projs.i = ptabs.iter().position(|thread_value| *thread_value == pcur).unwrap_or(0);
    let labels: Vec<String> = ptabs.iter().map(ProjTab::label).collect();

    state
        .projs
        .draw(frame, row(2), "project", &labels, state.focus == Focus::Projects);
    let all = state.views_all();

    state.views.i = all.iter().position(|value| *value == state.view).unwrap_or(0);
    let labels: Vec<String> = all.iter().map(|value_value| state.view_label(*value_value)).collect();

    state
        .views
        .draw(frame, row(3), "show   ", &labels, state.focus == Focus::Views);
}

fn footer_line(state: &ThreadList) -> Option<Line<'static>> {
    if let Some(path) = &state.job_state.pending {
        let count_4 = path.ids.len();

        // threads with work not in the base: the question names it
        let open: Vec<String> = path
            .ids
            .iter()
            .filter_map(|id| {
                state
                    .cards
                    .iter()
                    .find(|character_value| &character_value.id == id)?
                    .land
                    .clone()
                    .filter(crate::domain::landed::Land::open)
            })
            .map(|line| line.badge())
            .collect();

        let ask_archive = match open.as_slice() {
            [] => format!(" archive {count_4} thread(s)? "),
            [one] if count_4 == 1 => format!(" {one}: archive anyway? "),
            _ => format!(" archive {count_4} thread(s), {} open (work not landed)? ", open.len()),
        };

        let (ask, why) = match path.act {
            Act::Archive => (
                ask_archive,
                "hosts stop, worktrees go (uncommitted files → git stash), branches stay",
            ),
            _ => (
                format!(" delete {count_4} thread(s) + worktree(s)? "),
                "uncommitted files → git stash, pushed branches go",
            ),
        };

        Some(Line::from(vec![
            Span::styled(ask, theme::pill().add_modifier(Modifier::BOLD)),
            Span::styled(" enter: yes · esc: no  ", theme::accent()),
            Span::styled(why, theme::muted()),
        ]))
    } else {
        draw_thread_list_body(state)
    }
}

fn form_bounds(state: &Form, area: Rect) -> Rect {
    let width_value = area.width.saturating_sub(4).min(76);
    let body = 2
        + 1
        + state.projs.len()
        + 1
        + state.links.len().max(1)
        + 1
        + state.agents.len()
        + 2
        + usize::from(state.error.is_some());
    let height_value = (body as u16 + 2).min(area.height);

    Rect {
        x: area.x + (area.width - width_value) / 2,
        y: area.y + 1.min(area.height.saturating_sub(height_value)),
        width: width_value,
        height: height_value,
    }
}

fn draw_form_frame(frame: &mut Frame, area_value: Rect) -> Rect {
    frame.render_widget(Clear, area_value);
    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(theme::accent())
        .title(Span::styled(" edit thread ", theme::accent_bold()))
        .style(theme::text());
    let inner = block.inner(area_value);

    frame.render_widget(block, area_value);

    Rect {
        x: inner.x + 1,
        width: inner.width.saturating_sub(2),
        ..inner
    }
}

struct ProjectRowFrame<'reference_0, 'reference_1, 'reference_2> {
    frame: &'reference_0 mut Frame<'reference_1>,
    y: &'reference_2 mut u16,
}
