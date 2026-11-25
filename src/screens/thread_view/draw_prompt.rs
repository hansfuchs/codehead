use super::{Frame, Pane, Paragraph, Rect, Span, Style, View, theme};

impl View {
    pub(super) fn draw_prompt(
        &mut self,
        frame: &mut Frame,
        input: Rect,
        bubble: Rect,
        search: Rect,
    ) -> Option<Vec<String>> {
        // the input bubble: its top border the header's bottom line (├ ┴ ┤)
        // the bubble's border: the accent while the prompt takes the keys (the focused pane),
        // the shell color in shell mode
        let keys = (self.panes.pane == Pane::Chat || self.panes.prompt_focus)
            && self.agents.agents_focus.is_none()
            && self.menu.is_none();

        let edge = if self.shell_mode() {
            theme::fg(theme::shell())
        } else if keys {
            theme::accent()
        } else {
            theme::fg(theme::pal().surface1)
        };

        draw_bubble_border(frame, bubble, edge);

        self.draw_ask_hint(frame, bubble);

        self.layout.input_rect = input;
        let menu_open = self.menu.is_some();

        // an open ask with the focus: 0 cursor in the prompt (its keys go to the ask)
        let ask_keys = !self.thread.asks.is_empty() && self.ask.focus && !self.ask.sent;
        let prompt_keys = self.panes.pane == Pane::Chat || self.panes.prompt_focus;

        self.prompt.input.render(
            frame,
            self.layout.input_rect,
            !menu_open && !ask_keys && prompt_keys && !self.find.focus,
        );

        if search.height > 0 {
            self.find.render(frame, search);
        }

        let comp_cmds = if self.comp_on() { Some(self.shell_cmds()) } else { None };

        if let Some(cmds) = &comp_cmds {
            self.prompt
                .comp
                .render_suggestion(frame, &self.prompt.input, cmds, self.layout.input_rect);
        }

        comp_cmds
    }
    pub(super) fn draw_ask_hint(&mut self, frame: &mut Frame, bubble: Rect) {
        // asks: what the keys do now, in the bubble's bottom border
        if !self.thread.asks.is_empty() && self.prompt.input.is_empty() && self.ask.focus {
            draw_ask_hint_label(frame, bubble);
        }
    }
}

fn draw_ask_hint_label(frame: &mut Frame<'_>, bubble: Rect) {
    let label = " ↑↓ · 1-9: pick · enter: answer ";
    let lw = label.chars().count() as u16;

    if bubble.width > lw + 6 {
        frame.render_widget(
            Paragraph::new(Span::styled(label, theme::accent())),
            Rect {
                x: bubble.right() - 3 - lw,
                y: bubble.bottom().saturating_sub(1),
                width: lw,
                height: 1,
            },
        );
    }
}

fn draw_bubble_border(frame: &mut Frame, bubble: Rect, edge: Style) {
    let buf = frame.buffer_mut();
    let (left, right) = (bubble.x, bubble.right().saturating_sub(1));

    for x in left..=right {
        let top = buf[(x, bubble.y)].symbol().to_string();

        let sym = match top.as_str() {
            _ if x == left => "┌",
            _ if x == right => "┐",
            _ => "─",
        };

        buf[(x, bubble.y)].reset();
        buf[(x, bubble.y)].set_symbol(sym).set_style(edge);
        buf[(x, bubble.bottom().saturating_sub(1))].reset();
        buf[(x, bubble.bottom().saturating_sub(1))]
            .set_symbol(if x == left {
                "└"
            } else if x == right {
                "┘"
            } else {
                "─"
            })
            .set_style(edge);
    }

    for y in bubble.y + 1..bubble.bottom().saturating_sub(1) {
        buf[(left, y)].set_symbol("│").set_style(edge);
        buf[(right, y)].set_symbol("│").set_style(edge);
    }
}
