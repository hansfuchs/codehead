use super::{
    Constraint, Focus, Frame, Layout, Line, Modifier, Paragraph, ProjectsTab, Rect, Span, Wrap, projects, theme,
};

impl ProjectsTab {
    pub(crate) fn draw(&mut self, frame: &mut Frame, area: Rect, active: bool) {
        draw_projects_tab(self, frame, area, active);
    }
}

fn append_done_badge(project: &projects::Project, spans: &mut Vec<Span<'_>>) {
    if project.done {
        spans.push(Span::styled("  done", theme::fg(theme::pal().success())));
    }
}

fn draw_projects_tab(tab: &mut ProjectsTab, frame: &mut Frame, area: Rect, active: bool) {
    if area.height < 4 {
        return;
    }

    let [top, path_row, _, body] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
    ])
    .areas(area);

    let labels = repository_labels(&tab.repos);

    if labels.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::styled(
                "0 repos: start a thread in a git repo first",
                theme::muted(),
            )),
            top,
        );
        return;
    }

    draw_repositories(tab, frame, top, path_row, active, &labels);

    let [left, _, right] =
        Layout::horizontal([Constraint::Percentage(40), Constraint::Length(1), Constraint::Fill(1)]).areas(body);

    draw_project_list(tab, frame, left, active);

    if let Some(form) = tab.form.as_mut() {
        form.draw(frame, right);
    } else if let Some(project) = tab.items.get(tab.cursor) {
        draw_brief(frame, right, project);
    }
}

fn finish_front_matter(front: &mut bool, i: usize, line: &str) {
    if *front && i > 0 && line.trim() == "---" {
        *front = false;
    }
}

fn repository_labels(repos: &[std::path::PathBuf]) -> Vec<String> {
    repos
        .iter()
        .map(|repo| {
            repo.file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
        })
        .collect()
}

fn draw_repositories(
    tab: &mut ProjectsTab,
    frame: &mut Frame,
    top: Rect,
    path_row: Rect,
    active: bool,
    labels: &[String],
) {
    tab.tabs.i = tab.repo_i;
    tab.tabs
        .draw(frame, top, "repo", labels, active && tab.focus == Focus::Repos);

    if let Some(repo) = tab.repo() {
        let dir = projects::dir(repo);

        frame.render_widget(
            Paragraph::new(Line::styled(
                format!("      {}", crate::infra::paths::tilde(&dir.display().to_string())),
                theme::muted(),
            )),
            path_row,
        );
    }
}

fn draw_project_list(tab: &mut ProjectsTab, frame: &mut Frame, left: Rect, active: bool) {
    tab.list = Rect {
        height: left.height.saturating_sub(2),
        ..left
    };
    let mut lines: Vec<Line> = Vec::new();

    if tab.items.is_empty() {
        lines.push(Line::styled("0 projects · n: new project", theme::muted()));
        lines.push(Line::styled(
            "1 project per feature or goal, e.g. `website refinement`",
            theme::muted(),
        ));
    }

    for (i, project) in tab.items.iter().enumerate() {
        let selected = i == tab.cursor && tab.focus == Focus::List;

        let style = if selected && active {
            theme::pill().add_modifier(Modifier::BOLD)
        } else if project.done {
            theme::muted()
        } else {
            theme::text()
        };

        let threads = tab.thread_count(&project.slug);
        let mut spans = vec![Span::styled(format!(" {} ", project.title), style)];

        spans.push(Span::styled(
            format!("  {threads} thread{}", if threads == 1 { "" } else { "s" }),
            theme::muted(),
        ));

        append_done_badge(project, &mut spans);

        lines.push(Line::from(spans));
    }

    frame.render_widget(Paragraph::new(lines), tab.list);
}

impl super::Form {
    fn draw(&mut self, frame: &mut Frame, right: Rect) {
        // like the new thread view: a label above each field, the input on the panel color
        let [title_label, title_input, _, body_label, body_input] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .areas(right);

        let label = form_label;

        let head = if self.slug.is_none() {
            "new project · title"
        } else {
            "title"
        };

        frame.render_widget(Paragraph::new(label(head, self.on_title)), title_label);
        frame.render_widget(Paragraph::new("").style(theme::panel()), title_input);
        self.title.render(
            frame,
            Rect {
                x: title_input.x + 1,
                width: title_input.width.saturating_sub(2),
                ..title_input
            },
            self.on_title,
        );
        frame.render_widget(Paragraph::new(label("brief", !self.on_title)), body_label);
        frame.render_widget(Paragraph::new("").style(theme::panel()), body_input);
        self.body.render(
            frame,
            Rect {
                x: body_input.x + 1,
                width: body_input.width.saturating_sub(2),
                ..body_input
            },
            !self.on_title,
        );
    }
}

fn draw_brief(frame: &mut Frame, right: Rect, project: &projects::Project) {
    let text = std::fs::read_to_string(&project.path).unwrap_or_default();

    // the front matter: muted; headings: accent
    let mut front = text.starts_with("---");
    let mut lines: Vec<Line> = Vec::new();

    for (i, line) in text.lines().enumerate() {
        let style = if front {
            theme::muted()
        } else if line.starts_with('#') {
            theme::accent_bold()
        } else {
            theme::text()
        };

        lines.push(Line::styled(line.to_string(), style));

        finish_front_matter(&mut front, i, line);
    }

    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), right);
}

fn form_label(text: &str, focused: bool) -> Line<'static> {
    Line::styled(
        text.to_string(),
        if focused { theme::accent_bold() } else { theme::muted() },
    )
}
