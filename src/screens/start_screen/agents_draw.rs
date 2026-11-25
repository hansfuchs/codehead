use super::{Agent, App, CARD_H, Duration, Field, Frame, Line, Paragraph, Rect, Span, agents, sprites, theme};

impl App {
    pub(super) fn agent_preview(&mut self, agent: &Agent) -> Vec<Line<'static>> {
        let name = agent.name.clone();

        self.thread_list
            .extra_reads
            .request(name.clone(), move || agents::show_extra(&name));
        let extra = self.agent_list.apreview.get(&agent.name).cloned().unwrap_or_default();

        let kv = |key: &str, value: String| {
            Line::from(vec![
                Span::styled(format!("{key:<9}"), theme::muted()),
                Span::raw(value),
            ])
        };

        let side = vec![
            vec![],
            vec![Span::styled(format!("   {}", agent.name), theme::accent_bold())],
            vec![Span::styled(format!("   {}", agent.harness_model()), theme::muted())],
        ];
        let sprite = agents::sprite(agent);
        let mut out = sprites::lines(&sprite, agents::color(agent.color.as_deref()), side);

        draw_agent_profile(self, agent, kv, &mut out);

        if agent.name == self.cfg.agent {
            out.push(Line::styled(
                "default agent: takes over threads of deleted agents",
                theme::accent(),
            ));
        }

        let path_value = agents::parse_profile(agent.prompt.as_deref(), None);

        for (head, body) in [("voice", &path_value.voice), ("system prompt", &path_value.prompt)] {
            agent_preview_body(&mut out, head, body);
        }

        out.push(Line::raw(""));
        out.extend(
            extra
                .into_iter()
                .map(|line_value| Line::styled(line_value, theme::muted())),
        );
        let uses = self.uses(agent);

        out.push(Line::raw(""));
        out.push(Line::styled(format!("threads: {}", uses.len()), theme::bold()));

        for user in uses.iter().take(30) {
            agent_preview_live(&mut out, user);
        }

        out
    }

    /// Shared by the agents tab and task selection.
    pub(super) fn agent_grid(&self, width: u16) -> crate::ui::cards::Grid {
        let text_w = crate::ui::cards::text_width(
            self.agent_list
                .agents
                .iter()
                .flat_map(|agent| self.agent_card_text(agent))
                .collect::<Vec<_>>()
                .iter(),
        );

        crate::ui::cards::tight_grid(
            &self.avisible(),
            crate::ui::cards::card_width(text_w, true),
            CARD_H,
            width,
        )
    }

    /// 1 card grid per harness (A→Z), as many columns as fit.
    pub(super) fn draw_agents(&mut self, frame: &mut Frame, area: Rect) {
        self.draw_agent_grid(frame, area, false);
    }

    pub(super) fn draw_agent_grid(&mut self, frame: &mut Frame, area: Rect, task: bool) {
        let group = self.agent_grid(area.width);

        let mut cursor = if task {
            self.compose.pick_cursor
        } else {
            self.agent_list.acursor
        };

        let mut offset = if task {
            self.compose.pick_offset
        } else {
            self.agent_list.aoffset
        };

        if !group.places.iter().any(|&(i, _, _)| i == cursor) {
            cursor = group.places.first().map_or(0, |path| path.0);
        }

        if task {
            self.compose.pick_cursor = cursor;
        } else {
            self.agent_list.acursor = cursor;
        }

        if let Some(&(_, _, top)) = group.places.iter().find(|&&(i, _, _)| i == cursor) {
            offset = crate::ui::cards::scroll(&group, top, CARD_H, offset, area.height as usize);
        }

        offset = offset.min(group.height.saturating_sub(area.height as usize));

        if task {
            self.compose.pick_offset = offset;
        } else {
            self.agent_list.aoffset = offset;
        }

        let focused = !self.tabs_focus
            && if task {
                self.compose.field == Field::Agents
            } else {
                !self.agent_list.afilter_editing
            };

        let cards = crate::ui::cards::draw_grid(frame, area, &group, CARD_H, offset, |file, result, i| {
            draw_agent_grid_agent(self, task, cursor, focused, file, result, i);
        });

        let places = group.places;

        if places.is_empty() {
            draw_agent_grid_msg(self, frame, area);
        }

        if task {
            self.compose.pick_places = places;
            self.preview.regions.menu_rows = cards;
        } else {
            self.agent_list.aplace = places;
            self.agent_list.acards = cards;
        }
    }

    /// Agents tab card rows: name, harness · model, effort, threads (+ live).
    pub(super) fn agent_card_text(&self, agent: &Agent) -> [String; 4] {
        let [name, tech, effort] = self.card_text(agent);
        let uses: &[crate::domain::agents::Use] = self
            .agent_list
            .usage
            .as_ref()
            .and_then(|user| user.get(&agent.name))
            .map_or(&[], Vec::as_slice);
        let live = uses.iter().filter(|user_value| !user_value.live.is_empty()).count();

        let threads = if live > 0 {
            format!("{} threads · {live} live", uses.len())
        } else {
            format!("{} threads", uses.len())
        };

        [name, tech, effort, threads]
    }

    /// Agents tab card: like a task card, 0 pick mark; the default agent named in the border.
    pub(super) fn draw_agent_card(&self, frame: &mut Frame, area: Rect, agent: &Agent, cur: bool) {
        let title = (agent.name == self.cfg.agent).then(|| Span::styled(" default ", theme::fg(theme::pal().info())));
        let look = crate::ui::cards::Look {
            cur,
            lit: false,
            border: Some(crate::ui::cards::border(cur, false, theme::pal().accent)),
            title,
            bg: None,
        };

        crate::ui::cards::draw(frame, area, agent, &self.agent_card_text(agent), look);
    }
}

fn agent_preview_body(out: &mut Vec<Line<'static>>, head: &str, body: &str) {
    out.push(Line::raw(""));
    out.push(Line::styled(head.to_string(), theme::bold()));

    if body.is_empty() {
        out.push(Line::styled("-", theme::muted()));
    }

    out.extend(body.lines().map(|line| Line::raw(format!("  {line}"))));
}

fn agent_preview_live(out: &mut Vec<Line<'static>>, user: &agents::Use) {
    let live = if user.live.is_empty() {
        Span::raw("")
    } else {
        Span::styled(" live", theme::bold().fg(theme::pal().progress()))
    };

    out.push(Line::from(vec![
        Span::raw(format!("  {}", user.title)),
        live,
        Span::styled(format!(" {}", user.state), theme::muted()),
    ]));
}

fn draw_agent_profile(
    app_state: &mut App,
    agent: &Agent,
    kv: impl Fn(&str, String) -> Line<'static>,
    out: &mut Vec<Line<'static>>,
) {
    out.extend([
        Line::raw(""),
        kv("harness", agent.harness_model()),
        kv("model", agents::model_of(agent)),
        kv("effort", format!("{} (global)", app_state.agent_list.effort)),
        kv("mode", agent.mode.clone()),
        kv("color", agent.color.clone().unwrap_or_else(|| "-".into())),
        kv("style", {
            let path = agents::parse_profile(agent.prompt.as_deref(), None);

            match path
                .style
                .as_deref()
                .and_then(|id| app_state.agent_list.styles.iter().find(|text| text.id == id))
            {
                Some(text_value) => text_value.name.clone(),
                None if agent.prompt.as_deref().is_some_and(agents::written_by_us) => "none".into(),
                None => format!(
                    "{} (global)",
                    app_state.agent_list.global_style.clone().unwrap_or_else(|| "-".into())
                ),
            }
        }),
        kv(
            "seen",
            agent.last_seen.map_or("never".into(), |thread| {
                crate::domain::notes::ago(std::time::UNIX_EPOCH + Duration::from_secs(thread.max(0) as u64))
            }),
        ),
    ]);
}

fn draw_agent_grid_msg(app_state: &mut App, frame: &mut Frame<'_>, area: Rect) {
    let msg = if app_state.agent_list.agents.is_empty() {
        &format!("0 agents. {} → n: new agent", crate::infra::config::menu_key().label)
    } else {
        "0 agents match the filter"
    };

    frame.render_widget(Paragraph::new(Line::styled(msg, theme::muted())), area);
}

fn draw_agent_grid_agent(
    app_state: &mut App,
    task: bool,
    cursor: usize,
    focused: bool,
    file: &mut Frame<'_>,
    result: Rect,
    i: usize,
) {
    let agent = &app_state.agent_list.agents[i];

    if task {
        app_state.draw_card(file, result, agent, focused && i == cursor);
    } else {
        app_state.draw_agent_card(file, result, agent, focused && i == cursor);
    }
}
