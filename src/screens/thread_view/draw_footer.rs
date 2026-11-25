use super::{Duration, Frame, Line, Pane, Paragraph, Rect, Span, View, theme};

impl View {
    pub(super) fn draw_footer(&mut self, frame: &mut Frame, foot: Rect) {
        // footer: keys or toast right
        let right = match &self.toast {
            Some((toast, at)) if at.elapsed() < Duration::from_secs(3) => {
                Line::from(Span::styled(format!("{toast} "), theme::fg(theme::pal().info())))
            }
            _ if self.agents.agents_focus.is_some() => {
                Line::from(theme::hints("←→ agent · ↑↓ · enter · 1-3 · tab: next · esc: prompt "))
            }
            _ if self.panes.pane == Pane::Changes && !self.panes.prompt_focus && self.checkout_state.repo_focus => {
                Line::from(theme::hints("←→ repo · ↓ / enter / esc: the changes · tab: next "))
            }
            _ if self.panes.pane == Pane::Changes && !self.panes.prompt_focus => {
                Line::from(theme::hints(self.panes.changes.hints()))
            }
            _ if self.panes.pane == Pane::Merge && !self.panes.prompt_focus => {
                Line::from(theme::hints(&self.merge_state.merge.hints()))
            }
            _ if self.panes.pane == Pane::Files && !self.panes.prompt_focus => Line::from(theme::hints(
                "j k · ctrl+f filter · pgup pgdn: scroll · e: IDE · S S: share · tab: next · ctrl+.: actions ",
            )),
            _ if self.panes.pane != Pane::Chat => Line::from(theme::hints(
                "tab: next · esc: back to the view · ctrl+.: actions · F1-F4: views ",
            )),
            _ => Line::from(theme::hints(&format!(
                "{}: menu · ctrl+.: actions · tab: next · ctrl+f: search · F1-F4: views · esc: interrupt · esc esc: codehead ",
                crate::infra::config::menu_key().label
            ))),
        };

        // the mode the keys are in: a badge left, the keys right of it
        let badge = self
            .mode()
            .map(|(label, risky)| theme::mode_badge(label, risky))
            .unwrap_or_default();
        let badge_w = (badge.iter().map(|span| span.content.chars().count()).sum::<usize>() as u16).min(foot.width);

        frame.render_widget(Paragraph::new(Line::from(badge)), Rect { width: badge_w, ..foot });
        frame.render_widget(
            Paragraph::new(right).right_aligned(),
            Rect {
                x: foot.x + badge_w,
                width: foot.width - badge_w,
                ..foot
            },
        );
    }
}
