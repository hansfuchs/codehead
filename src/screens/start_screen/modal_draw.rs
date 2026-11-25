use super::{
    App, Block, BorderType, Borders, Clear, Constraint, Field, Frame, Layout, Line, Modal, Modifier, Paragraph, Rect,
    Screen, Span, Style, Tab, UnicodeWidthStr, hint, theme,
};

impl App {
    pub(super) fn draw_footer(&mut self, frame: &mut Frame, area: Rect) {
        let mut spans = vec![Span::raw(" ")];

        // a filter takes the letters: its mode badge first
        if self.compose.screen == Screen::Pick
            && (self.note_list.filter_editing || self.thread_list.tlist.filtering || self.agent_list.afilter_editing)
        {
            spans.extend(theme::mode_badge("filter", false));
        }

        let cmd = crate::infra::config::command_key().to_string();

        if !self.tabs_focus && self.compose.screen == Screen::Pick && self.tab == Tab::Merge {
            spans.extend(theme::hints(&self.merge_tab.hints()));
        }

        let prompt_hints = [
            ("@", "agents"),
            (cmd.as_str(), "commands"),
            ("dir/", "paths"),
            ("ctrl+v", "image"),
            ("↑↓", "field"),
            ("⌘/ctrl+⏎", "start"),
            ("esc esc", "quit"),
        ];
        let hints = self.footer_hints(&prompt_hints);

        for (key, line) in hints {
            spans.extend(hint(key, line));
        }

        frame.render_widget(Paragraph::new(Line::from(spans)), area);
        let mut right = Vec::new();

        if let Some((msg, _, err)) = &self.overlays.toast {
            draw_footer_body(&mut right, msg, *err);
        }

        right.push(Span::styled(crate::infra::config::menu_key().label, theme::accent()));
        right.push(Span::styled(" menu ", theme::muted()));
        frame.render_widget(Paragraph::new(Line::from(right).right_aligned()), area);
    }

    fn footer_hints<'hint>(
        &'hint self,
        prompt_hints: &'hint [(&'hint str, &'hint str)],
    ) -> &'hint [(&'hint str, &'hint str)] {
        if !self.tabs_focus && self.compose.screen == Screen::Pick && self.tab == Tab::Merge {
            return &[];
        }

        if self.tabs_focus {
            return &[("←→", "tab"), ("↓ / tab", "content"), ("esc esc", "quit")];
        }

        if self.compose.screen == Screen::Compose {
            return self.compose_footer_hints(prompt_hints);
        }

        pick_footer_hints(self)
    }
    fn compose_footer_hints<'hint>(
        &'hint self,
        prompt_hints: &'hint [(&'hint str, &'hint str)],
    ) -> &'hint [(&'hint str, &'hint str)] {
        if self.compose.field == Field::Agents {
            return &[
                ("↑↓←→", "move"),
                ("space", "pick"),
                ("ctrl+.", "flow"),
                ("enter", "notes"),
                ("⌘/ctrl+⏎", "start"),
                ("tab", "tabs row"),
            ];
        }

        if let Some(hints) = dropdown_footer_hints(self) {
            return hints;
        }

        self.field_footer_hints(prompt_hints)
    }
    fn field_footer_hints<'hint>(
        &'hint self,
        prompt_hints: &'hint [(&'hint str, &'hint str)],
    ) -> &'hint [(&'hint str, &'hint str)] {
        match self.compose.screen {
            Screen::Compose if self.compose.field == Field::Repo => {
                &[("enter", "pick repo"), ("↑↓", "field"), ("⌘/ctrl+⏎", "start")]
            }
            Screen::Compose if self.compose.field == Field::Where => {
                &[("enter", "choices"), ("↑↓", "field"), ("⌘/ctrl+⏎", "start")]
            }
            Screen::Compose if self.compose.field == Field::Projects => {
                &[("enter", "pick projects"), ("↑↓", "field"), ("⌘/ctrl+⏎", "start")]
            }
            Screen::Compose if self.compose.field == Field::Opts => &[
                ("space", "check / uncheck"),
                ("↑↓", "box · field"),
                ("⌘/ctrl+⏎", "start"),
            ],
            Screen::Compose if self.compose.field == Field::Drafts => &[
                ("←→", "button"),
                ("enter", "press"),
                ("↓", "prompt"),
                ("⌘/ctrl+⏎", "start"),
                ("ctrl+s", "save draft"),
                ("F1-F7", "tabs"),
            ],
            Screen::Compose if self.compose.field == Field::Prompt => prompt_hints,
            Screen::Compose if self.compose.field == Field::Notes => &[
                ("enter", "pick notes"),
                ("x", "drop"),
                ("↑↓", "field"),
                ("⌘/ctrl+⏎", "start"),
                ("tab", "tabs row"),
            ],
            _ => &[
                ("↑↓", "field"),
                ("⌘/ctrl+⏎", "start"),
                ("ctrl+s", "save draft"),
                ("ctrl+.", "actions"),
                ("F1-F7", "tabs"),
                ("esc esc", "quit"),
            ],
        }
    }

    pub(super) fn frame(frame: &mut Frame, area: Rect, title: &str, width: u16, height: u16) -> Rect {
        let width_value = width.min(area.width.saturating_sub(2));
        let height_value = height.min(area.height.saturating_sub(2));
        let area_value = Rect {
            x: area.x + (area.width - width_value) / 2,
            y: area.y + (area.height - height_value) / 2,
            width: width_value,
            height: height_value,
        };

        // a modal: the screen behind it fades
        theme::dim(frame.buffer_mut(), area);
        frame.render_widget(Clear, area_value);
        let block = Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Plain)
            .border_style(theme::accent())
            .title(Span::styled(format!(" {title} "), theme::accent_bold()))
            .style(theme::text());
        let inner = block.inner(area_value);

        frame.render_widget(block, area_value);

        Rect {
            x: inner.x + 1,
            width: inner.width.saturating_sub(2),
            ..inner
        }
    }

    pub(super) fn draw_modal(&mut self, frame: &mut Frame, area: Rect) {
        let Some(modal) = &self.overlays.modal else { return };

        match modal {
            Modal::Confirm {
                title, question, yes, ..
            } => {
                self.preview.regions.buttons = confirm_buttons(frame, area, title, question, *yes);
            }
            Modal::Agent(_) => {
                self.draw_agent_form(frame, area);
                self.draw_form_pick(frame, area);
            }
            Modal::Info { title, lines, scroll } => {
                let width_value = lines.iter().map(ratatui::prelude::Line::width).max().unwrap_or(20) as u16 + 6;
                let height = lines.len() as u16 + 4;
                let inner = Self::frame(frame, area, title, width_value.clamp(40, 120), height);
                let [body, foot] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);

                frame.render_widget(Paragraph::new(lines.clone()).scroll((*scroll, 0)), body);
                frame.render_widget(Paragraph::new(Line::from(hint("enter", "close"))), foot);
            }
        }
    }
}

fn draw_footer_body(right: &mut Vec<Span<'_>>, msg: &String, err: bool) {
    right.push(Span::styled(
        format!("{msg}   "),
        Style::default().fg(if err {
            theme::pal().error()
        } else {
            theme::pal().success()
        }),
    ));
}

fn dropdown_footer_hints(app_state: &App) -> Option<&[(&str, &str)]> {
    match app_state.compose.screen {
        Screen::Compose if app_state.repository.where_menu.is_some() => {
            Some(&[("↑↓", "move"), ("type", "filter"), ("enter", "pick"), ("esc", "close")])
        }
        Screen::Compose if app_state.project_selection.proj_menu.is_some() => Some(&[
            ("↑↓", "move"),
            ("type", "filter"),
            ("enter", "pick"),
            ("tab", "add"),
            ("esc", "close"),
        ]),
        Screen::Compose
            if app_state
                .flow_selection
                .flow_menu
                .as_ref()
                .is_some_and(|message| message.sub.is_some()) =>
        {
            Some(&[("↑↓", "step"), ("enter", "start here"), ("← esc", "flows")])
        }
        Screen::Compose if app_state.flow_selection.flow_menu.is_some() => Some(&[
            ("↑↓", "flow"),
            ("enter →", "its steps"),
            ("a-z", "jump"),
            ("0", "0 flow"),
            ("esc", "close"),
        ]),
        Screen::Compose if app_state.repository.repo_pick.is_some() => Some(&[
            ("↑↓", "dir"),
            ("←→", "out · in"),
            ("type", "filter"),
            ("~", "home"),
            ("enter", "pick"),
            ("esc", "close"),
        ]),
        Screen::Compose if app_state.draft_selection.drafts_menu.is_some() => {
            Some(&[("↑↓", "move"), ("enter", "load"), ("d", "delete"), ("esc", "back")])
        }
        _ => None,
    }
}

fn pick_footer_hints(app_state: &App) -> &[(&str, &str)] {
    match app_state.compose.screen {
        Screen::Pick if app_state.thread_list.tlist.filtering => {
            &[("type", "filter"), ("↑↓", "move"), ("enter / esc", "back to list")]
        }
        Screen::Pick if app_state.note_list.filter_editing => {
            &[("type", "filter"), ("↑↓", "move"), ("enter", "done"), ("esc", "clear")]
        }
        Screen::Pick if app_state.tab == Tab::Notes && app_state.cfg.copy_mode => &[
            ("↑↓", "move"),
            ("←→", "fold"),
            ("space", "select"),
            ("enter", "copy"),
            ("esc", "quit"),
        ],
        Screen::Pick if app_state.tab == Tab::Notes => &[
            ("↑↓", "move"),
            ("←→", "fold"),
            ("space", "select"),
            ("enter", "confirm"),
            ("esc", "new"),
            ("tab", "back"),
        ],
        Screen::Pick if app_state.tab == Tab::Threads => &[
            ("↑↓", "move"),
            ("space", "select"),
            ("a", "archive"),
            ("e", "edit"),
            ("ctrl+o", "actions"),
            ("enter", "open"),
            ("ctrl+f", "filter"),
        ],
        Screen::Pick if app_state.tab == Tab::Settings => app_state.settings.as_ref().map_or(&[], |text| text.help()),
        Screen::Pick if app_state.tab == Tab::Flows => app_state.flows_tab.help(),
        Screen::Pick if app_state.tab == Tab::Projects => app_state.project_selection.projects_tab.hints(),
        Screen::Pick if app_state.agent_list.afilter_editing => {
            &[("type", "filter"), ("↑↓", "move"), ("enter / esc", "back to cards")]
        }
        _ => &[
            ("↑↓←→", "move"),
            ("ctrl+f", "filter"),
            ("enter", "edit"),
            ("F1-F7", "tabs"),
        ],
    }
}

fn confirm_buttons(frame: &mut Frame<'_>, area: Rect, title: &str, question: &str, yes: bool) -> Vec<(Rect, bool)> {
    let width = (question.width() as u16 + 8).clamp(44, 90);
    let inner = App::frame(frame, area, title, width, 7);
    let [_, query, _, bytes] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    frame.render_widget(Paragraph::new(Line::styled(question, theme::text()).centered()), query);
    let labels = [(true, "  yes (y)  "), (false, "  no (n/esc)  ")];
    let total: u16 = labels.iter().map(|(_, line)| line.width() as u16).sum::<u16>() + 2;
    let mut x = bytes.x + bytes.width.saturating_sub(total) / 2;
    let mut buttons = Vec::new();

    for (value, line_value) in labels {
        let focused = value == yes;

        let style = if focused {
            theme::pill().add_modifier(Modifier::BOLD)
        } else {
            Style::default().bg(theme::pal().surface0).fg(theme::pal().text)
        };

        let area_value = Rect {
            x,
            y: bytes.y,
            width: line_value.width() as u16,
            height: 1,
        };

        frame.render_widget(Paragraph::new(line_value).style(style), area_value);
        buttons.push((area_value, value));
        x += area_value.width + 2;
    }

    buttons
}
