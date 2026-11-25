use super::task_layout::TaskLayout;
use super::{
    Agent, App, Field, Frame, Line, Modifier, Paragraph, Rect, Span, Style, UnicodeWidthStr, agents, theme, tilde,
};
use std::fmt::Write as _;

impl App {
    pub(super) fn draw_compose(&mut self, frame: &mut Frame, area: Rect) {
        let label = |text: &str, focused: bool| draw_compose_body(text, focused);

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
        let order = self.compose.picks.iter().position(|path| *path == agent.name);

        let mark = match order {
            Some(count) if self.compose.picks.len() > 1 => format!(" ◼ {} ", count + 1),
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
            Some(fl) if start > 0 => format!("◆ {} › {}", fl.name, start + 1),
            Some(fl) => format!("◆ {}", fl.name),
            None => "0 flow".into(),
        };

        [name, tech, effort, flow]
    }
}

fn draw_compose_body(text: &str, focused: bool) -> Line<'static> {
    Line::styled(
        text.to_string(),
        if focused { theme::accent_bold() } else { theme::muted() },
    )
}

fn draw_task_drafts_branch(cur: bool) -> Style {
    if cur {
        theme::pill().add_modifier(Modifier::BOLD)
    } else {
        theme::pill_dim()
    }
}

fn draw_task_notes_body(lines: &mut Vec<Line<'_>>, character: &crate::infra::claude::Session) {
    lines.push(Line::from(vec![
        Span::styled(
            if character.thread.is_some() {
                "  ↳ thread "
            } else {
                "  ↳ claude code "
            },
            theme::accent(),
        ),
        Span::raw(character.title.clone()),
        Span::styled(
            format!(" · last response, {} lines", character.last.lines().count()),
            theme::muted(),
        ),
    ]));
}

fn draw_task_drafts_hint(frame: &mut Frame<'_>, r_drafts: Rect, btn: usize, area_2: Rect) {
    let hint = if btn == 0 {
        "enter: list of unsent tasks"
    } else {
        "enter: keep the task for later, start over"
    };

    let hx = area_2.right() + 2;
    let area_3 = Rect {
        x: hx,
        width: r_drafts.right().saturating_sub(hx),
        height: 1,
        ..r_drafts
    };

    frame.render_widget(Paragraph::new(Line::styled(hint, theme::muted())), area_3);
}

fn draw_task_repository_head(
    chip: Style,
    frame_value: &mut Frame<'_>,
    area_4: Rect,
    name: &str,
    on: bool,
    value: &str,
    text: String,
) -> Rect {
    let head = format!(" {name:<7}");
    let x = area_4.x + head.width() as u16;
    let value = format!(" {value} ");
    let area = Rect {
        x: x.min(area_4.right().saturating_sub(1)),
        y: area_4.y,
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

    frame_value.render_widget(Paragraph::new(Line::from(spans)), area_4);

    area
}

fn draw_task_drafts(app_state: &mut App, frame: &mut Frame<'_>, r_drafts: Rect) {
    let on = app_state.compose.field == Field::Drafts;
    let btn = app_state.draft_btn();
    let st = |cur: bool| draw_task_drafts_branch(cur);
    let mut x = r_drafts.x;

    app_state.preview.regions.drafts = Rect::default();

    if !app_state.draft_selection.drafts.is_empty() {
        let text = format!(" view drafts ({}) ", app_state.draft_selection.drafts.len());
        let area_value = Rect {
            x,
            width: (text.width() as u16).min(r_drafts.width),
            height: 1,
            ..r_drafts
        };

        frame.render_widget(Paragraph::new(text).style(st(on && btn == 0)), area_value);
        app_state.preview.regions.drafts = area_value;
        x = area_value.right() + 2;
    }

    let text = " save as draft ";
    let area_2 = Rect {
        x,
        width: (text.width() as u16).min(r_drafts.right().saturating_sub(x)),
        height: 1,
        ..r_drafts
    };

    frame.render_widget(Paragraph::new(text).style(st(on && btn == 1)), area_2);
    app_state.preview.regions.save_draft = area_2;

    if on && app_state.draft_selection.drafts_menu.is_none() {
        draw_task_drafts_hint(frame, r_drafts, btn, area_2);
    }
}

fn draw_task_title(
    app_state: &mut App,
    frame: &mut Frame<'_>,
    label: impl Fn(&str, bool) -> Line<'static>,
    l_title: Rect,
    i_title: Rect,
) {
    frame.render_widget(
        Paragraph::new(label(
            if app_state.compose.title.is_empty() {
                "title · optional: the prompt's 1st line"
            } else {
                "title"
            },
            app_state.compose.field == Field::Title,
        )),
        l_title,
    );
    frame.render_widget(Paragraph::new("").style(theme::panel()), i_title);
    app_state.compose.title.render(
        frame,
        Rect {
            x: i_title.x + 1,
            width: i_title.width.saturating_sub(2),
            ..i_title
        },
        app_state.compose.field == Field::Title,
    );
}

fn draw_task_repository(
    app_state: &mut App,
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
        app_state.compose.field,
        Field::Repo | Field::Projects | Field::Where | Field::Opts
    );

    frame.render_widget(Paragraph::new(label("where", any)), l_where);
    let chip = Style::default().fg(theme::pal().text).add_modifier(Modifier::BOLD);

    // a row: the sub-label, the chip (its area back), the text
    let row = |frame_value: &mut Frame, area_4: Rect, name: &str, on: bool, value: String, text: String| -> Rect {
        draw_task_repository_head(chip, frame_value, area_4, name, on, &value, text)
    };

    let (repo_value, repo_text) = match &app_state.repository.repo {
        Some(result) if app_state.clone_first().is_some() => (
            result.name.clone(),
            format!("{} · 0 clone yet", tilde(&result.path.display().to_string())),
        ),
        Some(result_value) => (
            result_value.name.clone(),
            tilde(&result_value.path.display().to_string()),
        ),
        None => (
            "0 git repo".into(),
            format!(
                "works in {} · enter: pick a repo",
                tilde(&app_state.cfg.cwd.display().to_string())
            ),
        ),
    };

    app_state.preview.regions.repo_chip = row(
        frame,
        repo_row,
        "repo",
        app_state.compose.field == Field::Repo,
        repo_value,
        repo_text,
    );
    app_state.preview.regions.repo_row = repo_row;

    draw_task_branch(app_state, frame, proj_h, proj_row, info, row);

    // opts: ◼ checkout, ◻ stash below it, in the column of the sub-labels
    draw_task_options(app_state, frame, opts_h, opts);
}

fn draw_task_branch(
    app_state: &mut App,
    frame: &mut Frame<'_>,
    proj_h: u16,
    proj_row: Rect,
    info: Rect,
    row: impl Fn(&mut Frame<'_>, Rect, &str, bool, String, String) -> Rect,
) {
    if proj_h > 0 {
        draw_task_branch_count(app_state, frame, proj_row, &row);
    }

    if !app_state.repository.wheres.is_empty() {
        app_state.preview.regions.where_chip = row(
            frame,
            info,
            "branch",
            app_state.compose.field == Field::Where,
            app_state.where_chip(),
            app_state.where_text(),
        );
        app_state.preview.regions.where_row = info;
    }
}

fn draw_task_branch_count(
    app_state: &mut App,
    frame: &mut Frame<'_>,
    proj_row: Rect,
    row: &impl Fn(&mut Frame<'_>, Rect, &str, bool, String, String) -> Rect,
) {
    let on = app_state.compose.field == Field::Projects;
    let count = app_state.task_projects().len();

    let value = if count == 0 {
        "0 projects".to_string()
    } else {
        app_state.proj_value()
    };

    let text = if count == 0 {
        "projects tab (F3): new project".to_string()
    } else if app_state.project_selection.proj_picks.is_none() {
        format!("agents assign projects · {count} in this repo")
    } else {
        format!("{count} in this repo")
    };

    app_state.preview.regions.proj_chip = row(frame, proj_row, "project", on, value, text);
    app_state.preview.regions.proj_row = proj_row;
}

fn draw_task_options(app_state: &mut App, frame: &mut Frame<'_>, opts_h: u16, opts: Rect) {
    if opts_h > 0 {
        draw_task_options_text(app_state, frame, opts);
    }
}

fn draw_task_options_text(app_state: &mut App, frame: &mut Frame<'_>, opts: Rect) {
    let on = app_state.compose.field == Field::Opts;
    let x = opts.x;

    for (i, (checked, enabled), label) in [
        (0, app_state.checkout_box(), "checkout selected branch"),
        (1, app_state.stash_box(), "stash conflicting local changes (git stash)"),
    ] {
        if i as u16 >= opts.height {
            break;
        }

        let text = format!(" {} {label} ", if checked { "◼" } else { "◻" });
        let width = (text.width() as u16).min((opts.x + opts.width).saturating_sub(x));

        // the box under the cursor: accent, like the other fields (disabled: dim on accent)
        let style = match (enabled, on && app_state.repository.opt_i == i) {
            (true, true) => theme::pill().add_modifier(Modifier::BOLD),
            (false, true) => theme::pill().add_modifier(Modifier::DIM),
            (false, false) => Style::default().fg(theme::pal().overlay0),
            (true, false) if checked => Style::default().fg(theme::pal().text),
            (true, false) => theme::muted(),
        };

        let area_5 = Rect {
            x,
            y: opts.y + i as u16,
            width,
            height: 1,
        };

        frame.render_widget(Paragraph::new(text).style(style), area_5);
        app_state.preview.regions.opts.push((area_5, i));
    }
}

fn draw_task_agents(
    app_state: &mut App,
    frame: &mut Frame<'_>,
    label: impl Fn(&str, bool) -> Line<'static>,
    l_agents: Rect,
    r_agents: Rect,
) {
    let focused = app_state.compose.field == Field::Agents;

    let agents_label = match app_state.compose.picks.len() {
        0 => "agents · 0 picked".to_string(),
        1 => "agents · 1 picked".to_string(),
        count_value => format!("agents · {count_value} picked: 1st starts the thread, the others join"),
    };

    frame.render_widget(Paragraph::new(label(&agents_label, focused)), l_agents);
    app_state.preview.regions.task_agents = r_agents;
    app_state.draw_agent_grid(frame, r_agents, true);
}

fn draw_task_notes(
    app_state: &mut App,
    frame: &mut Frame<'_>,
    label: impl Fn(&str, bool) -> Line<'static>,
    n_notes: usize,
    l_notes: Rect,
    notes: Rect,
) {
    let focused = app_state.compose.field == Field::Notes;

    let mut notes_label = if n_notes == 0 {
        "notes · 0 picked".to_string()
    } else {
        format!("notes · {n_notes} picked, listed above the prompt")
    };

    if !app_state.compose.carry.is_empty() {
        let _ = write!(
            notes_label,
            " · {} context (threads tab: c)",
            app_state.compose.carry.len()
        );
    }

    if focused {
        notes_label.push_str(" · enter: pick · x: drop all");
    }

    frame.render_widget(Paragraph::new(label(&notes_label, focused)), l_notes);
    app_state.preview.regions.notes_label = l_notes;
    let mut lines: Vec<Line> = app_state
        .note_list
        .selected
        .iter()
        .take(5)
        .map(|path| Line::from(vec![Span::styled("  ◼ ", theme::accent()), Span::raw(path.clone())]))
        .collect();

    if n_notes > 5 {
        lines.push(Line::styled(format!("  … {} more", n_notes - 5), theme::muted()));
    }

    for character in app_state.compose.carry.iter().take(5) {
        draw_task_notes_body(&mut lines, character);
    }

    frame.render_widget(Paragraph::new(lines), notes);
}

fn draw_task_prompt(
    app_state: &mut App,
    frame: &mut Frame<'_>,
    label: impl Fn(&str, bool) -> Line<'static>,
    l_prompt: Rect,
    i_prompt: Rect,
) {
    frame.render_widget(
        Paragraph::new(label("prompt", app_state.compose.field == Field::Prompt)),
        l_prompt,
    );
    frame.render_widget(Paragraph::new("").style(theme::panel()), i_prompt);
    let inner = Rect {
        x: i_prompt.x + 1,
        width: i_prompt.width.saturating_sub(2),
        ..i_prompt
    };

    app_state.compose.prompt.marks = app_state.agent_names();
    app_state
        .compose
        .prompt
        .render(frame, inner, app_state.compose.field == Field::Prompt);
}
