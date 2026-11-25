use super::{App, Block, Borders, Frame, Line, Paragraph, Rect, Span, Tab, Wrap, theme};

impl App {
    pub(super) fn preview_content(&mut self, area: Rect, thin: bool) -> (String, Vec<Line<'static>>, bool) {
        let width = area.width.saturating_sub(if thin { 1 } else { 3 }).max(20);

        if let Some(value) = preview_agent(self) {
            return value;
        }

        if let Some(value) = preview_thread(self) {
            return value;
        }

        let Some(row) = self.current().cloned() else {
            let lines = vec![
                Line::styled(format!("0 notes in {}", self.cfg.notes.display()), theme::muted()),
                Line::styled(
                    "add .md files there, or press enter to start without notes",
                    theme::muted(),
                ),
            ];

            return ("notes".into(), lines, true);
        };

        if row.is_dir() {
            return preview_directory(self, &row);
        }

        let path = self.note_list.notes.abs(row.path());

        let lines = match self.preview.previews.get(path, width) {
            Some(line) => line.clone(),
            None => vec![Line::styled("…", theme::muted())],
        };

        (row.path().to_string(), lines, false)
    }
}

pub(super) fn draw_preview(app_state: &mut App, frame: &mut Frame<'_>, area: Rect) -> (Rect, Rect) {
    let (list, preview, thin) = app_state.split(area);

    app_state.preview.regions.list = list;

    let block = if thin {
        Block::new().borders(Borders::TOP).border_style(theme::line())
    } else {
        Block::new().borders(Borders::LEFT).border_style(theme::line())
    };

    let (title, lines, wrap) = app_state.preview_content(preview, thin);
    let block = block.title(Span::styled(format!(" {title} "), theme::muted()));
    let inner = block.inner(preview);
    let inner = Rect {
        x: inner.x + 1,
        width: inner.width.saturating_sub(1),
        ..inner
    };

    app_state.preview.regions.preview = inner;
    frame.render_widget(block, preview);
    let max = (lines.len() as u16).saturating_sub(inner.height);

    app_state.preview.pscroll = app_state.preview.pscroll.min(max);
    let mut path = Paragraph::new(lines).scroll((app_state.preview.pscroll, 0));

    if wrap {
        path = path.wrap(Wrap { trim: false });
    }

    frame.render_widget(path, inner);

    (list, inner)
}

pub(super) fn preview_thread(app_state: &mut App) -> Option<(String, Vec<Line<'static>>, bool)> {
    if app_state.tab != Tab::Threads {
        return None;
    }

    if let Some(text) = app_state.current_session() {
        return Some((
            format!("claude · {}", text.place()),
            crate::ui::session_preview::preview(text),
            true,
        ));
    }

    let Some(width_value) = app_state.current_wt() else {
        let msg = empty_threads(app_state);

        return Some(("threads".into(), vec![Line::styled(msg, theme::muted())], true));
    };

    let key = width_value.key();
    let read = width_value.clone();

    // 0 dir (a card not read yet): the header only, 0 git in the start dir
    if !width_value.path.is_empty() {
        app_state
            .thread_list
            .detail_reads
            .request(key.clone(), move || crate::ui::worktree_preview::preview(&read));
    }

    let lines = app_state
        .thread_list
        .tpreview
        .get(&key)
        .cloned()
        .unwrap_or_else(|| crate::ui::worktree_preview::preview_header(&width_value));

    Some((width_value.label(), lines, true))
}

pub(super) fn preview_agent(app_state: &mut App) -> Option<(String, Vec<Line<'static>>, bool)> {
    if matches!(app_state.tab, Tab::Agents | Tab::New) {
        let Some(agent) = app_state.current_agent().cloned() else {
            return Some((
                "agents".into(),
                vec![Line::styled("0 agents. n: new agent", theme::muted())],
                true,
            ));
        };

        return Some((agent.name.clone(), app_state.agent_preview(&agent), true));
    }

    None
}

pub(super) fn preview_content_body(
    app_state: &mut App,
    row: &crate::domain::notes::Row,
    lines: &mut Vec<Line<'_>>,
    path_value: &str,
) {
    lines.push(Line::from(vec![
        Span::raw(format!("  {}", &path_value[row.path().len()..])),
        Span::styled(
            format!("  {}", app_state.note_list.notes.age(path_value)),
            theme::muted(),
        ),
    ]));
}

pub(super) fn preview_directory(
    app_state: &mut App,
    row: &crate::domain::notes::Row,
) -> (String, Vec<Line<'static>>, bool) {
    let files = app_state.note_list.notes.inside(row.path());
    let mut lines = vec![
        Line::from(vec![
            Span::styled(row.path().to_string(), theme::bold()),
            Span::styled(format!("  {} notes", files.len()), theme::muted()),
        ]),
        Line::raw(""),
    ];
    let mut files = files;

    files.sort_by_key(|file| std::cmp::Reverse(app_state.note_list.notes.mtime(file)));

    for path_value in files {
        preview_content_body(app_state, row, &mut lines, &path_value);
    }

    (row.path().to_string(), lines, true)
}

pub(super) fn empty_threads(app_state: &App) -> &'static str {
    if app_state.thread_list.tlist.view.cards() {
        "0 threads"
    } else if app_state.cfg.repo.is_some() {
        "0 worktrees"
    } else {
        "not in a git repo: 0 worktrees"
    }
}
