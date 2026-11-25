use super::{App, Block, Borders, Frame, Line, Paragraph, Rect, Span, Tab, Wrap, theme};

impl App {
    pub(super) fn preview_content(&mut self, area: Rect, thin: bool) -> (String, Vec<Line<'static>>, bool) {
        let width = area.width.saturating_sub(if thin { 1 } else { 3 }).max(20);

        if let Some(preview) = preview_agent(self) {
            return preview;
        }

        if let Some(preview) = preview_thread(self) {
            return preview;
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
            Some(cached) => cached.clone(),
            None => vec![Line::styled("…", theme::muted())],
        };

        (row.path().to_string(), lines, false)
    }
}

pub(super) fn draw_preview(app: &mut App, frame: &mut Frame<'_>, area: Rect) -> (Rect, Rect) {
    let (list, preview, thin) = app.split(area);

    app.preview.regions.list = list;

    let block = if thin {
        Block::new().borders(Borders::TOP).border_style(theme::line())
    } else {
        Block::new().borders(Borders::LEFT).border_style(theme::line())
    };

    let (title, lines, wrap) = app.preview_content(preview, thin);
    let block = block.title(Span::styled(format!(" {title} "), theme::muted()));
    let inner = block.inner(preview);
    let inner = Rect {
        x: inner.x + 1,
        width: inner.width.saturating_sub(1),
        ..inner
    };

    app.preview.regions.preview = inner;
    frame.render_widget(block, preview);
    let max = (lines.len() as u16).saturating_sub(inner.height);

    app.preview.pscroll = app.preview.pscroll.min(max);
    let mut paragraph = Paragraph::new(lines).scroll((app.preview.pscroll, 0));

    if wrap {
        paragraph = paragraph.wrap(Wrap { trim: false });
    }

    frame.render_widget(paragraph, inner);

    (list, inner)
}

pub(super) fn preview_thread(app: &mut App) -> Option<(String, Vec<Line<'static>>, bool)> {
    if app.tab != Tab::Threads {
        return None;
    }

    if let Some(session) = app.current_session() {
        return Some((
            format!("claude · {}", session.place()),
            crate::ui::session_preview::preview(session),
            true,
        ));
    }

    let Some(worktree) = app.current_wt() else {
        let msg = empty_threads(app);

        return Some(("threads".into(), vec![Line::styled(msg, theme::muted())], true));
    };

    let key = worktree.key();
    let read = worktree.clone();

    // 0 dir (a card not read yet): the header only, 0 git in the start dir
    if !worktree.path.is_empty() {
        app.thread_list
            .detail_reads
            .request(key.clone(), move || crate::ui::worktree_preview::preview(&read));
    }

    let lines = app
        .thread_list
        .tpreview
        .get(&key)
        .cloned()
        .unwrap_or_else(|| crate::ui::worktree_preview::preview_header(&worktree));

    Some((worktree.label(), lines, true))
}

pub(super) fn preview_agent(app: &mut App) -> Option<(String, Vec<Line<'static>>, bool)> {
    if matches!(app.tab, Tab::Agents | Tab::New) {
        let Some(agent) = app.current_agent().cloned() else {
            return Some((
                "agents".into(),
                vec![Line::styled("0 agents. n: new agent", theme::muted())],
                true,
            ));
        };

        return Some((agent.name.clone(), app.agent_preview(&agent), true));
    }

    None
}

pub(super) fn push_note_line(app: &mut App, row: &crate::domain::notes::Row, lines: &mut Vec<Line<'_>>, note: &str) {
    lines.push(Line::from(vec![
        Span::raw(format!("  {}", &note[row.path().len()..])),
        Span::styled(format!("  {}", app.note_list.notes.age(note)), theme::muted()),
    ]));
}

pub(super) fn preview_directory(app: &mut App, row: &crate::domain::notes::Row) -> (String, Vec<Line<'static>>, bool) {
    let files = app.note_list.notes.inside(row.path());
    let mut lines = vec![
        Line::from(vec![
            Span::styled(row.path().to_string(), theme::bold()),
            Span::styled(format!("  {} notes", files.len()), theme::muted()),
        ]),
        Line::raw(""),
    ];
    let mut files = files;

    files.sort_by_key(|file| std::cmp::Reverse(app.note_list.notes.mtime(file)));

    for note in files {
        push_note_line(app, row, &mut lines, &note);
    }

    (row.path().to_string(), lines, true)
}

pub(super) fn empty_threads(app: &App) -> &'static str {
    if app.thread_list.tlist.view.cards() {
        "0 threads"
    } else if app.cfg.repo.is_some() {
        "0 worktrees"
    } else {
        "not in a git repo: 0 worktrees"
    }
}
