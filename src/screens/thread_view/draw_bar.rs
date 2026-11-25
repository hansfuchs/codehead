use super::{Frame, Line, Modifier, Pane, Paragraph, Rect, Span, View, home_label, theme};

struct BarRight {
    link: Option<(String, String)>,
    link_w: usize,
    badge: Vec<Span<'static>>,
    badge_w: usize,
    used: usize,
}
impl View {
    pub(super) fn draw_bar(&mut self, frame: &mut Frame, top: Rect) {
        let mut spans: Vec<Span<'static>> = Vec::new();

        self.layout.bar_rects.clear();
        let mut x = top.x + 2;

        self.bar_home(top, &mut x, &mut spans);

        self.bar_panes(top, &mut x, &mut spans);

        spans.push(Span::styled("│ ", theme::line()));
        let used: usize = spans
            .iter()
            .map(|text| unicode_width::UnicodeWidthStr::width(text.content.as_ref()))
            .sum();

        let link = self.bar_link();

        let link_w = link
            .as_ref()
            .map_or(0, |(thread, _)| unicode_width::UnicodeWidthStr::width(thread.as_str()));

        let badge = self.bar_badge();

        let badge_w = usize::from(crate::features::notifications::width(&badge));

        let badge_w = if top.width as usize > used + link_w + badge_w + 24 {
            badge_w
        } else {
            0
        };

        let room = usize::from(top.width.saturating_sub(4)).saturating_sub(used + link_w + badge_w + 1);

        spans.push(Span::styled(
            format!("{} ", crate::ui::thread_card::fit(&self.title, room)),
            theme::bold(),
        ));
        frame.render_widget(
            Paragraph::new(Line::from(spans)),
            Rect {
                x: top.x + 2,
                width: top.width.saturating_sub(2),
                ..top
            },
        );

        draw_bar_right(
            self,
            frame,
            top,
            BarRight {
                link,
                link_w,
                badge,
                badge_w,
                used,
            },
        );
    }
    pub(super) fn bar_home(&mut self, top: Rect, x: &mut u16, spans: &mut Vec<Span<'static>>) {
        // home (the start screen), with the count of the other threads that need the owner
        let (home, waiting) = home_label(&crate::domain::notifications::unseen(Some(&self.id)));
        let hw = unicode_width::UnicodeWidthStr::width(home.as_str()) as u16;

        self.layout.home_rect = Rect {
            x: *x,
            y: top.y,
            width: hw,
            height: 1,
        };
        *x += hw + 1;
        spans.push(Span::styled(
            home,
            if waiting {
                theme::fg(theme::pal().attention()).add_modifier(Modifier::BOLD)
            } else {
                theme::muted()
            },
        ));
        spans.push(Span::styled("│ ", theme::line()));
        *x += 2;
    }
    pub(super) fn bar_panes(&mut self, top: Rect, x: &mut u16, spans: &mut Vec<Span<'static>>) {
        for path in Pane::ALL {
            bar_panes_label(self, top, x, spans, path);
        }
    }

    pub(super) fn pane_label(&mut self, path: Pane) -> String {
        match path {
            Pane::Chat if !self.thread.asks.is_empty() => format!(" chat ?{} ", self.thread.asks.len()),
            Pane::Chat => " chat ".to_string(),
            Pane::Changes => " changes ".to_string(),
            Pane::Merge => count_label("merge", self.merge_state.merge.count()),
            Pane::Files => count_label("files", self.panes.files.count()),
        }
    }
    pub(super) fn bar_link(&mut self) -> Option<(String, String)> {
        // portless: the worktree's live URL on the right (a click opens it)
        self.poll_urls();
        self.urls_state.url_rect = None;

        let more = if self.urls_state.urls.len() > 1 {
            format!(" +{}", self.urls_state.urls.len() - 1)
        } else {
            String::new()
        };

        self.urls_state.urls.first().map(|user| {
            let host = user
                .trim_start_matches("https://")
                .trim_start_matches("http://")
                .trim_end_matches('/');

            (format!("↗ {host}{more} "), user.clone())
        })
    }
    pub(super) fn bar_badge(&mut self) -> Vec<Span<'static>> {
        // notifications of the other threads; this thread's while scrolled back: ↓ N new
        let mut badge = crate::features::notifications::badge(&crate::domain::notifications::unseen(Some(&self.id)));
        let here = crate::domain::notifications::unseen_of(&self.id).len();

        if here > 0 && self.stream.back > 0 {
            badge.insert(
                0,
                Span::styled(format!("↓ {here} new  "), theme::bold().fg(theme::pal().info())),
            );
        }

        badge
    }
}

fn draw_bar_right_area(
    frame: &mut Frame<'_>,
    top: Rect,
    link_w: usize,
    badge: Vec<Span<'static>>,
    badge_w: usize,
    used: usize,
) {
    let lw = if top.width as usize > link_w + used + 4 {
        link_w
    } else {
        0
    };

    let area = Rect {
        x: top.right().saturating_sub((lw + badge_w + 1) as u16),
        width: badge_w as u16,
        ..top
    };

    frame.render_widget(Paragraph::new(Line::from(badge)), area);
}

fn count_label(label: &str, count: usize) -> String {
    if count == 0 {
        format!(" {label} ")
    } else {
        format!(" {label} {count} ")
    }
}

fn bar_panes_label(view: &mut View, top: Rect, x: &mut u16, spans: &mut Vec<Span<'static>>, path: Pane) {
    let label = view.pane_label(path);

    let st = if path == view.panes.pane {
        theme::pill()
    } else {
        theme::muted()
    };

    let width = unicode_width::UnicodeWidthStr::width(label.as_str()) as u16;

    view.layout.bar_rects.push((
        Rect {
            x: *x,
            y: top.y,
            width,
            height: 1,
        },
        path,
    ));
    *x += width + 1;
    spans.push(Span::styled(label, st));
    spans.push(Span::raw(" "));
}

fn draw_bar_right(view: &mut View, frame: &mut Frame, top: Rect, right: BarRight) {
    let BarRight {
        link,
        link_w,
        badge,
        badge_w,
        used,
    } = right;

    if let Some((text, url)) = link
        && top.width as usize > link_w + used + 4
    {
        let area = Rect {
            x: top.right() - link_w as u16,
            width: link_w as u16,
            ..top
        };

        frame.render_widget(
            Paragraph::new(Span::styled(
                text,
                theme::fg(theme::pal().info()).add_modifier(Modifier::UNDERLINED),
            )),
            area,
        );
        view.urls_state.url_rect = Some((area, url));
    }

    if badge_w > 0 {
        draw_bar_right_area(frame, top, link_w, badge, badge_w, used);
    }
}
