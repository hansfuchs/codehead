use super::{App, Frame, Line, Paragraph, Rect, Span, Style, theme};

impl App {
    /// The rows of the views worktrees (0 thread) / claude (sessions) in AREA.
    pub(super) fn draw_threads(&mut self, frame: &mut Frame, area: Rect) {
        let height = area.height as usize;

        let Some(items) = &self.thread_list.worktrees else {
            return;
        };

        let vis = self.tvisible();

        if vis.is_empty() {
            draw_threads_msg(self, frame, area);
            return;
        }

        let hint = if self.thread_list.tlist.view == crate::features::thread_list::View::Claude {
            "enter: new thread, last response as context · c: context"
        } else {
            "enter: new task there · d: delete worktree"
        };

        frame.render_widget(
            Paragraph::new(Line::styled(format!(" {hint}"), theme::muted())),
            Rect { height: 1, ..area },
        );
        let area = Rect {
            y: area.y + 1,
            height: area.height.saturating_sub(1),
            ..area
        };
        let height_value = height.saturating_sub(1);

        self.thread_list.tcursor = self.thread_list.tcursor.min(vis.len() - 1);

        if self.thread_list.tcursor < self.thread_list.toffset {
            self.thread_list.toffset = self.thread_list.tcursor;
        } else if self.thread_list.tcursor >= self.thread_list.toffset + height_value {
            self.thread_list.toffset = self.thread_list.tcursor + 1 - height_value;
        }

        self.thread_list.toffset = self.thread_list.toffset.min(vis.len().saturating_sub(height_value));
        let nw = items.len();
        let dim = self.tabs_focus
            || self.thread_list.tlist.focus != crate::features::thread_list::Focus::List
            || self.thread_list.tlist.modal();

        for (result, &i) in vis.iter().enumerate().skip(self.thread_list.toffset).take(height_value) {
            let rect = Rect {
                y: area.y + (result - self.thread_list.toffset) as u16,
                height: 1,
                ..area
            };
            let hl = result == self.thread_list.tcursor;
            let style = self.row_style(hl && !dim);
            let spans = thread_row_spans(self, i, nw, items, hl, dim);

            frame.render_widget(Paragraph::new(Line::from(spans)).style(style), rect);
        }
    }
}

fn thread_row_spans(
    app_state: &App,
    i: usize,
    nw: usize,
    items: &[super::Worktree],
    hl: bool,
    dim: bool,
) -> Vec<Span<'static>> {
    let on_pill = hl && !dim;

    let fg = |style_value: Style| {
        if on_pill {
            style_value.fg(theme::pal().on_accent)
        } else {
            style_value
        }
    };

    let muted = fg(theme::muted());

    if i >= nw {
        let character = &app_state.thread_list.sessions[i - nw];
        let ctx = app_state.in_context(&character.key());

        vec![
            Span::raw(" "),
            if ctx {
                Span::styled("↳ ", fg(theme::accent()))
            } else {
                Span::styled("› ", fg(theme::line()))
            },
            Span::styled(character.place(), fg(theme::bold())),
            Span::raw("  "),
            Span::styled(character.title.chars().take(60).collect::<String>(), fg(theme::text())),
            Span::styled(format!(" {}", character.when), muted),
        ]
    } else {
        let width = &items[i];
        let mut spans = vec![
            Span::raw(" "),
            Span::styled("› ", fg(theme::line())),
            Span::styled(width.branch.clone(), fg(theme::bold())),
            Span::styled(format!("  {}", width.path), muted),
        ];

        if width.unpushed > 0 {
            spans.push(Span::styled(
                format!(" {} unpushed", width.unpushed),
                fg(theme::fg(theme::pal().attention())),
            ));
        }

        spans.push(Span::styled(format!(" {}", width.when), muted));
        spans
    }
}

fn draw_threads_msg(app_state: &App, frame: &mut Frame<'_>, area: Rect) {
    let msg = if !app_state.thread_list.tlist.query().trim().is_empty() {
        "0 matches"
    } else if app_state.thread_list.tlist.view == crate::features::thread_list::View::Claude {
        "0 Claude Code sessions"
    } else {
        "0 worktrees without thread"
    };

    frame.render_widget(Paragraph::new(Line::styled(msg, theme::muted())), area);
}
