use super::{
    Constraint, Focus, Frame, Layout, Line, Modifier, Paragraph, ProjectsTab, Rect, Span, Wrap, projects, theme,
};

impl ProjectsTab {
    pub(crate) fn draw(&mut self, frame: &mut Frame, area: Rect, active: bool) {
        draw_projects_tab(self, frame, area, active);
    }
}

fn append_done_badge(path_value: &projects::Project, text_value: &mut Vec<Span<'_>>) {
    if path_value.done {
        text_value.push(Span::styled("  done", theme::fg(theme::pal().success())));
    }
}

fn draw_projects_tab(receiver_state: &mut ProjectsTab, frame: &mut Frame, area: Rect, active: bool) {
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

    let labels = repository_labels(&receiver_state.repos);

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

    draw_repositories(receiver_state, frame, top, path_row, active, &labels);

    let [left, _, right] =
        Layout::horizontal([Constraint::Percentage(40), Constraint::Length(1), Constraint::Fill(1)]).areas(body);

    draw_project_list(receiver_state, frame, left, active);

    if let Some(fm) = receiver_state.form.as_mut() {
        fm.draw(frame, right);
    } else if let Some(path_2) = receiver_state.items.get(receiver_state.cursor) {
        draw_brief(frame, right, path_2);
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
        .map(|result| {
            result
                .file_name()
                .map(|count_value| count_value.to_string_lossy().into_owned())
                .unwrap_or_default()
        })
        .collect()
}

fn draw_repositories(
    receiver_state: &mut ProjectsTab,
    frame: &mut Frame,
    top: Rect,
    path_row: Rect,
    active: bool,
    labels: &[String],
) {
    receiver_state.tabs.i = receiver_state.repo_i;
    receiver_state.tabs.draw(
        frame,
        top,
        "repo",
        labels,
        active && receiver_state.focus == Focus::Repos,
    );

    if let Some(result_value) = receiver_state.repo() {
        let dir = projects::dir(result_value);

        frame.render_widget(
            Paragraph::new(Line::styled(
                format!("      {}", crate::infra::paths::tilde(&dir.display().to_string())),
                theme::muted(),
            )),
            path_row,
        );
    }
}

fn draw_project_list(receiver_state: &mut ProjectsTab, frame: &mut Frame, left: Rect, active: bool) {
    receiver_state.list = Rect {
        height: left.height.saturating_sub(2),
        ..left
    };
    let mut lines: Vec<Line> = Vec::new();

    if receiver_state.items.is_empty() {
        lines.push(Line::styled("0 projects · n: new project", theme::muted()));
        lines.push(Line::styled(
            "1 project per feature or goal, e.g. `website refinement`",
            theme::muted(),
        ));
    }

    for (i, path_value) in receiver_state.items.iter().enumerate() {
        let cur = i == receiver_state.cursor && receiver_state.focus == Focus::List;

        let st = if cur && active {
            theme::pill().add_modifier(Modifier::BOLD)
        } else if path_value.done {
            theme::muted()
        } else {
            theme::text()
        };

        let count_2 = receiver_state.count(&path_value.slug);
        let mut text_value = vec![Span::styled(format!(" {} ", path_value.title), st)];

        text_value.push(Span::styled(
            format!("  {count_2} thread{}", if count_2 == 1 { "" } else { "s" }),
            theme::muted(),
        ));

        append_done_badge(path_value, &mut text_value);

        lines.push(Line::from(text_value));
    }

    frame.render_widget(Paragraph::new(lines), receiver_state.list);
}

impl super::Form {
    fn draw(&mut self, frame: &mut Frame, right: Rect) {
        // like the new thread view: a label above each field, the input on the panel color
        let [l_title, i_title, _, l_body, i_body] = Layout::vertical([
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

        frame.render_widget(Paragraph::new(label(head, self.on_title)), l_title);
        frame.render_widget(Paragraph::new("").style(theme::panel()), i_title);
        self.title.render(
            frame,
            Rect {
                x: i_title.x + 1,
                width: i_title.width.saturating_sub(2),
                ..i_title
            },
            self.on_title,
        );
        frame.render_widget(Paragraph::new(label("brief", !self.on_title)), l_body);
        frame.render_widget(Paragraph::new("").style(theme::panel()), i_body);
        self.body.render(
            frame,
            Rect {
                x: i_body.x + 1,
                width: i_body.width.saturating_sub(2),
                ..i_body
            },
            !self.on_title,
        );
    }
}

fn draw_brief(frame: &mut Frame, right: Rect, path_2: &projects::Project) {
    let text = std::fs::read_to_string(&path_2.path).unwrap_or_default();

    // the front matter: muted; headings: accent
    let mut front = text.starts_with("---");
    let mut lines: Vec<Line> = Vec::new();

    for (i, line) in text.lines().enumerate() {
        let st = if front {
            theme::muted()
        } else if line.starts_with('#') {
            theme::accent_bold()
        } else {
            theme::text()
        };

        lines.push(Line::styled(line.to_string(), st));

        finish_front_matter(&mut front, i, line);
    }

    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), right);
}

fn form_label(text: &str, on: bool) -> Line<'static> {
    Line::styled(text.to_string(), if on { theme::accent_bold() } else { theme::muted() })
}
