use super::task_layout::TaskLayout;
use super::{
    Agent, App, Field, Frame, Line, Modifier, Paragraph, Rect, Span, Style, UnicodeWidthStr, agents, theme, tilde,
};
use std::fmt::Write as _;

impl App {
    pub(super) fn draw_compose(&mut self, frame: &mut Frame, area: Rect) {
        let label = |text: &str, focused: bool| field_label(text, focused);

        let layout = self.task_layout(area);

        draw_task_drafts(self, frame, layout.r_drafts);

        draw_task_title(self, frame, label, layout.l_title, layout.i_title);

        draw_task_repository(self, frame, label, &layout);

        draw_task_agents(self, frame, label, layout.l_agents, layout.r_agents);
        draw_task_notes(self, frame, label, layout.n_notes, layout.l_notes, layout.notes);
        draw_task_prompt(self, frame, label, layout.l_prompt, layout.i_prompt);
    }

    /// Text rows of an agent card: name, harness · model, effort.
    pub(super) fn card_text(&self, agent: &Agent) -> [String; 3] {
        let tech = crate::ui::cards::tech(agent);
        let effort = crate::ui::cards::effort(agent, &self.agent_list.effort);

        [agent.name.clone(), tech, effort]
    }

    /// Agent card: box, sprite left, text right. Picked: border in the agent color, ◼ (+ order).
    /// Cursor: accent border.
    pub(super) fn draw_card(&self, frame: &mut Frame, area: Rect, agent: &Agent, cur: bool) {
        let color = agents::color(agent.color.as_deref());
        let order = self.compose.picks.iter().position(|pick| *pick == agent.name);

        let mark = match order {
            Some(index) if self.compose.picks.len() > 1 => format!(" ◼ {} ", index + 1),
            Some(_) => " ◼ ".to_string(),
            None => " ◻ ".to_string(),
        };

        let title = Span::styled(
            mark,
            if order.is_some() {
                Style::default().fg(color).add_modifier(Modifier::BOLD)
            } else {
                theme::muted()
            },
        );
        let look = crate::ui::cards::Look {
            cur,
            lit: order.is_some(),
            border: Some(crate::ui::cards::border(cur, order.is_some(), color)),
            title: Some(title),
            bg: None,
        };

        crate::ui::cards::draw(frame, area, agent, &self.task_card_text(agent), look);
    }

    /// New-thread card rows: name, harness · model, effort, the agent's flow.
    pub(super) fn task_card_text(&self, agent: &Agent) -> [String; 4] {
        let [name, tech, effort] = self.card_text(agent);
        let (slug, start) = self.flow_of(&agent.name);

        let flow = match self.flow_selection.flow_lib.flow(&slug).filter(|_| !slug.is_empty()) {
            Some(flow) if start > 0 => format!("◆ {} › {}", flow.name, start + 1),
            Some(flow) => format!("◆ {}", flow.name),
            None => "0 flow".into(),
        };

        [name, tech, effort, flow]
    }
}

fn field_label(text: &str, focused: bool) -> Line<'static> {
    Line::styled(
        text.to_string(),
        if focused { theme::accent_bold() } else { theme::muted() },
    )
}

fn draft_button_style(cur: bool) -> Style {
    if cur {
        theme::pill().add_modifier(Modifier::BOLD)
    } else {
        theme::pill_dim()
    }
}

fn push_carry_line(lines: &mut Vec<Line<'_>>, session: &crate::infra::claude::Session) {
    lines.push(Line::from(vec![
        Span::styled(
            if session.thread.is_some() {
                "  ↳ thread "
            } else {
                "  ↳ claude code "
            },
            theme::accent(),
        ),
        Span::raw(session.title.clone()),
        Span::styled(
            format!(" · last response, {} lines", session.last.lines().count()),
            theme::muted(),
        ),
    ]));
}

fn draw_task_drafts_hint(frame: &mut Frame<'_>, r_drafts: Rect, btn: usize, save_area: Rect) {
    let hint = if btn == 0 {
        "enter: list of unsent tasks"
    } else {
        "enter: keep the task for later, start over"
    };

    let hx = save_area.right() + 2;
    let hint_area = Rect {
        x: hx,
        width: r_drafts.right().saturating_sub(hx),
        height: 1,
        ..r_drafts
    };

    frame.render_widget(Paragraph::new(Line::styled(hint, theme::muted())), hint_area);
}

fn draw_chip_row(
    chip: Style,
    frame: &mut Frame<'_>,
    row_area: Rect,
    name: &str,
    on: bool,
    value: &str,
    text: String,
) -> Rect {
    let head = format!(" {name:<7}");
    let x = row_area.x + head.width() as u16;
    let value = format!(" {value} ");
    let area = Rect {
        x: x.min(row_area.right().saturating_sub(1)),
        y: row_area.y,
        width: value.width() as u16,
        height: 1,
    };
    let spans = vec![
        Span::styled(head, if on { theme::accent_bold() } else { theme::muted() }),
        Span::styled(value, if on { chip.bg(theme::pal().surface0) } else { chip }),
        Span::styled(if on { "▾" } else { " " }, theme::accent()),
        Span::raw("  "),
        Span::styled(text, theme::muted()),
    ];

    frame.render_widget(Paragraph::new(Line::from(spans)), row_area);

    area
}

fn draw_task_drafts(app: &mut App, frame: &mut Frame<'_>, r_drafts: Rect) {
    let on = app.compose.field == Field::Drafts;
    let btn = app.draft_btn();
    let st = |cur: bool| draft_button_style(cur);
    let mut x = r_drafts.x;

    app.preview.regions.drafts = Rect::default();

    if !app.draft_selection.drafts.is_empty() {
        let text = format!(" view drafts ({}) ", app.draft_selection.drafts.len());
        let view_area = Rect {
            x,
            width: (text.width() as u16).min(r_drafts.width),
            height: 1,
            ..r_drafts
        };

        frame.render_widget(Paragraph::new(text).style(st(on && btn == 0)), view_area);
        app.preview.regions.drafts = view_area;
        x = view_area.right() + 2;
    }

    let text = " save as draft ";
    let save_area = Rect {
        x,
        width: (text.width() as u16).min(r_drafts.right().saturating_sub(x)),
        height: 1,
        ..r_drafts
    };

    frame.render_widget(Paragraph::new(text).style(st(on && btn == 1)), save_area);
    app.preview.regions.save_draft = save_area;

    if on && app.draft_selection.drafts_menu.is_none() {
        draw_task_drafts_hint(frame, r_drafts, btn, save_area);
    }
}

fn draw_task_title(
    app: &mut App,
    frame: &mut Frame<'_>,
    label: impl Fn(&str, bool) -> Line<'static>,
    l_title: Rect,
    i_title: Rect,
) {
    frame.render_widget(
        Paragraph::new(label(
            if app.compose.title.is_empty() {
                "title · optional: the prompt's 1st line"
            } else {
                "title"
            },
            app.compose.field == Field::Title,
        )),
        l_title,
    );
    frame.render_widget(Paragraph::new("").style(theme::panel()), i_title);
    app.compose.title.render(
        frame,
        Rect {
            x: i_title.x + 1,
            width: i_title.width.saturating_sub(2),
            ..i_title
        },
        app.compose.field == Field::Title,
    );
}

fn draw_task_repository(
    app: &mut App,
    frame: &mut Frame<'_>,
    label: impl Fn(&str, bool) -> Line<'static>,
    layout: &TaskLayout,
) {
    let TaskLayout {
        opts_h,
        proj_h,
        l_where,
        repo_row,
        proj_row,
        info,
        opts,
        ..
    } = *layout;

    let any = matches!(
        app.compose.field,
        Field::Repo | Field::Projects | Field::Where | Field::Opts
    );

    frame.render_widget(Paragraph::new(label("where", any)), l_where);
    let chip = Style::default().fg(theme::pal().text).add_modifier(Modifier::BOLD);

    // a row: the sub-label, the chip (its area back), the text
    let row = |frame: &mut Frame, row_area: Rect, name: &str, on: bool, value: String, text: String| -> Rect {
        draw_chip_row(chip, frame, row_area, name, on, &value, text)
    };

    let (repo_name, repo_text) = match &app.repository.repo {
        Some(repo) if app.clone_first().is_some() => (
            repo.name.clone(),
            format!("{} · 0 clone yet", tilde(&repo.path.display().to_string())),
        ),
        Some(repo) => (repo.name.clone(), tilde(&repo.path.display().to_string())),
        None => (
            "0 git repo".into(),
            format!(
                "works in {} · enter: pick a repo",
                tilde(&app.cfg.cwd.display().to_string())
            ),
        ),
    };

    app.preview.regions.repo_chip = row(
        frame,
        repo_row,
        "repo",
        app.compose.field == Field::Repo,
        repo_name,
        repo_text,
    );
    app.preview.regions.repo_row = repo_row;

    draw_task_branch(app, frame, proj_h, proj_row, info, row);

    // opts: ◼ checkout, ◻ stash below it, in the column of the sub-labels
    draw_task_options(app, frame, opts_h, opts);
}

fn draw_task_branch(
    app: &mut App,
    frame: &mut Frame<'_>,
    proj_h: u16,
    proj_row: Rect,
    info: Rect,
    row: impl Fn(&mut Frame<'_>, Rect, &str, bool, String, String) -> Rect,
) {
    if proj_h > 0 {
        draw_task_projects(app, frame, proj_row, &row);
    }

    if !app.repository.wheres.is_empty() {
        app.preview.regions.where_chip = row(
            frame,
            info,
            "branch",
            app.compose.field == Field::Where,
            app.where_chip(),
            app.where_text(),
        );
        app.preview.regions.where_row = info;
    }
}

fn draw_task_projects(
    app: &mut App,
    frame: &mut Frame<'_>,
    proj_row: Rect,
    row: &impl Fn(&mut Frame<'_>, Rect, &str, bool, String, String) -> Rect,
) {
    let on = app.compose.field == Field::Projects;
    let count = app.task_projects().len();

    let value = if count == 0 {
        "0 projects".to_string()
    } else {
        app.proj_chip_text()
    };

    let text = if count == 0 {
        "projects tab (F3): new project".to_string()
    } else if app.project_selection.proj_picks.is_none() {
        format!("agents assign projects · {count} in this repo")
    } else {
        format!("{count} in this repo")
    };

    app.preview.regions.proj_chip = row(frame, proj_row, "project", on, value, text);
    app.preview.regions.proj_row = proj_row;
}

fn draw_task_options(app: &mut App, frame: &mut Frame<'_>, opts_h: u16, opts: Rect) {
    if opts_h > 0 {
        draw_task_option_boxes(app, frame, opts);
    }
}

fn draw_task_option_boxes(app: &mut App, frame: &mut Frame<'_>, opts: Rect) {
    let on = app.compose.field == Field::Opts;
    let x = opts.x;

    for (i, (checked, enabled), label) in [
        (0, app.checkout_box(), "checkout selected branch"),
        (1, app.stash_box(), "stash conflicting local changes (git stash)"),
    ] {
        if i as u16 >= opts.height {
            break;
        }

        let text = format!(" {} {label} ", if checked { "◼" } else { "◻" });
        let width = (text.width() as u16).min((opts.x + opts.width).saturating_sub(x));

        // the box under the cursor: accent, like the other fields (disabled: dim on accent)
        let style = match (enabled, on && app.repository.opt_i == i) {
            (true, true) => theme::pill().add_modifier(Modifier::BOLD),
            (false, true) => theme::pill().add_modifier(Modifier::DIM),
            (false, false) => Style::default().fg(theme::pal().overlay0),
            (true, false) if checked => Style::default().fg(theme::pal().text),
            (true, false) => theme::muted(),
        };

        let area = Rect {
            x,
            y: opts.y + i as u16,
            width,
            height: 1,
        };

        frame.render_widget(Paragraph::new(text).style(style), area);
        app.preview.regions.opts.push((area, i));
    }
}

fn draw_task_agents(
    app: &mut App,
    frame: &mut Frame<'_>,
    label: impl Fn(&str, bool) -> Line<'static>,
    l_agents: Rect,
    r_agents: Rect,
) {
    let focused = app.compose.field == Field::Agents;

    let agents_label = match app.compose.picks.len() {
        0 => "agents · 0 picked".to_string(),
        1 => "agents · 1 picked".to_string(),
        count => format!("agents · {count} picked: 1st starts the thread, the others join"),
    };

    frame.render_widget(Paragraph::new(label(&agents_label, focused)), l_agents);
    app.preview.regions.task_agents = r_agents;
    app.draw_agent_grid(frame, r_agents, true);
}

fn draw_task_notes(
    app: &mut App,
    frame: &mut Frame<'_>,
    label: impl Fn(&str, bool) -> Line<'static>,
    n_notes: usize,
    l_notes: Rect,
    notes: Rect,
) {
    let focused = app.compose.field == Field::Notes;

    let mut notes_label = if n_notes == 0 {
        "notes · 0 picked".to_string()
    } else {
        format!("notes · {n_notes} picked, listed above the prompt")
    };

    if !app.compose.carry.is_empty() {
        let _ = write!(notes_label, " · {} context (threads tab: c)", app.compose.carry.len());
    }

    if focused {
        notes_label.push_str(" · enter: pick · x: drop all");
    }

    frame.render_widget(Paragraph::new(label(&notes_label, focused)), l_notes);
    app.preview.regions.notes_label = l_notes;
    let mut lines: Vec<Line> = app
        .note_list
        .selected
        .iter()
        .take(5)
        .map(|note| Line::from(vec![Span::styled("  ◼ ", theme::accent()), Span::raw(note.clone())]))
        .collect();

    if n_notes > 5 {
        lines.push(Line::styled(format!("  … {} more", n_notes - 5), theme::muted()));
    }

    for session in app.compose.carry.iter().take(5) {
        push_carry_line(&mut lines, session);
    }

    frame.render_widget(Paragraph::new(lines), notes);
}

fn draw_task_prompt(
    app: &mut App,
    frame: &mut Frame<'_>,
    label: impl Fn(&str, bool) -> Line<'static>,
    l_prompt: Rect,
    i_prompt: Rect,
) {
    frame.render_widget(
        Paragraph::new(label("prompt", app.compose.field == Field::Prompt)),
        l_prompt,
    );
    frame.render_widget(Paragraph::new("").style(theme::panel()), i_prompt);
    let inner = Rect {
        x: i_prompt.x + 1,
        width: i_prompt.width.saturating_sub(2),
        ..i_prompt
    };

    app.compose.prompt.marks = app.agent_names();
    app.compose
        .prompt
        .render(frame, inner, app.compose.field == Field::Prompt);
}
