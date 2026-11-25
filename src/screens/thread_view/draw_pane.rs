use super::{Frame, Line, Pane, Paragraph, Rect, Span, View, theme};

impl View {
    pub(super) fn draw_pane(&mut self, frame: &mut Frame, area: Rect, changes_left: Option<Rect>) {
        if self.panes.pane == Pane::Files {
            self.panes.files.draw(frame, area);
        } else if self.panes.pane == Pane::Merge {
            draw_merge_pane(self, frame, area);
        } else if self.panes.pane == Pane::Chat {
            self.draw_stream(frame, area);
        } else {
            self.draw_changes_pane(frame, area, changes_left);
        }
    }

    pub(super) fn draw_changes_pane(&mut self, frame: &mut Frame, area: Rect, changes_left: Option<Rect>) {
        // wide: the stream in the chat column, the changes left (alt+\ toggles)
        let area = if let Some(left) = changes_left {
            self.draw_stream(frame, area);
            left
        } else {
            area
        };

        let area = self.draw_mini_chat(frame, area);

        let area = self.draw_repo_tabs(frame, area);

        self.layout.view_rect = area;
        self.panes.changes.draw(frame, area);

        if let Some(note) = self.panes.changes.take_note() {
            self.toast(note);
        }

        // the CI fix: the failed log to the agents of the last message
        if let Some(log) = self.panes.changes.take_send() {
            let to = self.default_to().join(", ");

            self.send_prompt(&log);
            self.toast(format!("CI log sent to {to}"));
        }
    }
    pub(super) fn draw_mini_chat(&mut self, frame: &mut Frame, area: Rect) -> Rect {
        // narrow (0 split): the chat above the prompt, under the changes (its title row folds it)
        self.mini_chat_state.mini_rect = Rect::default();

        if self.layout.stream_rect == area && area.height >= 16 {
            draw_mini_chat_rows(self, frame, area)
        } else {
            area
        }
    }

    pub(super) fn draw_repo_tabs(&mut self, frame: &mut Frame, area: Rect) -> Rect {
        // the repo tabs on top, a gap below them
        if self.checkout_state.checkouts.is_empty() {
            self.load_checkouts();
        }

        if area.height > 6 && !self.checkout_state.checkout_labels.is_empty() {
            self.checkout_state.checkout_tabs.i = self.checkout_state.checkout;
            let labels = self.checkout_state.checkout_labels.clone();

            // the accent pill: the tabs have the keys; else the picked repo as a panel pill
            self.checkout_state.checkout_tabs.draw(
                frame,
                Rect { height: 1, ..area },
                "repo",
                &labels,
                self.checkout_state.repo_focus && !self.panes.prompt_focus,
            );
            Rect {
                y: area.y + 2,
                height: area.height - 2,
                ..area
            }
        } else {
            area
        }
    }
}

fn draw_merge_pane(view: &mut View, frame: &mut Frame<'_>, area: Rect) {
    view.merge_sources(false);
    view.merge_state.merge.draw(frame, area, !view.panes.prompt_focus);

    if let Some(note) = view.merge_state.merge.note.take() {
        view.toast(note);
    }
}

fn draw_mini_chat_rows(view: &mut View, frame: &mut Frame<'_>, area: Rect) -> Rect {
    let rows = if view.mini_chat_state.mini_chat {
        (area.height / 3).clamp(4, 14)
    } else {
        0
    };

    let title = Rect {
        y: area.bottom() - rows - 1,
        height: 1,
        ..area
    };
    let chat = Rect {
        y: title.bottom(),
        height: rows,
        ..area
    };

    let label = if view.mini_chat_state.mini_chat {
        "▾ chat · t: fold "
    } else {
        "▸ chat · t: open "
    };

    let busy = if view.thread.busy.is_empty() {
        String::new()
    } else {
        format!("· {} at work ", view.thread.busy.len())
    };

    let text = format!("─ {label}{busy}");
    let fill = usize::from(title.width).saturating_sub(unicode_width::UnicodeWidthStr::width(text.as_str()));

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(text, theme::muted()),
            Span::styled("─".repeat(fill), theme::line()),
        ])),
        title,
    );
    view.mini_chat_state.mini_rect = title;
    view.layout.stream_rect = chat;

    if rows > 0 {
        view.draw_stream(frame, chat);
    }

    Rect {
        height: title.y - area.y,
        ..area
    }
}
