use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::engine::flows::{self, Flow, Resolved};
use crate::features::flows::flow_rail::{self, Arc, Row};
use crate::ui::{dropdown, theme};

pub(super) struct FlowMenuDraw {
    pub(super) bounds: Rect,
    pub(super) list_area: Rect,
    pub(super) list_rows: Vec<(Rect, usize)>,
    pub(super) cursor: usize,
    pub(super) selected_flow: usize,
    pub(super) selected_step: usize,
    pub(super) steps: Option<(usize, usize)>,
}

pub(super) struct FlowPreview {
    flow: Flow,
    rows: Vec<Row>,
    arcs: Vec<Arc>,
    head: Line<'static>,
    labels_width: usize,
}

impl FlowPreview {
    pub(super) fn new(flow: Flow, library: &flows::Library) -> Self {
        let nodes = flows::resolve(&flow, library);
        let rows = crate::features::flows::rail_rows(&nodes);
        let arcs = flow_rail::arcs(&nodes, |_| 0);
        let head = crate::features::flows::head_line(&flow, &nodes);
        let labels_width = labels_width(&nodes);

        Self {
            flow,
            rows,
            arcs,
            head,
            labels_width,
        }
    }

    pub(super) fn place(&self, menu: &FlowMenuDraw) -> Option<Rect> {
        let want = (self
            .head
            .width()
            .max(self.flow.description.width())
            .max(titles_width(&self.rows) + 8 + usize::from(flow_rail::arcs_width(&self.arcs)) + self.labels_width)
            + 4) as u16;
        let row_y = menu
            .list_rows
            .iter()
            .find(|(_, index)| *index == menu.cursor)
            .map_or(menu.list_area.y + 1, |(area, _)| area.y);
        let right = menu.bounds.right().saturating_sub(menu.list_area.right());

        let width = if right >= 50 {
            want.clamp(40, right)
        } else {
            want.max(40)
        };

        dropdown::place_beside_max(
            menu.list_area,
            row_y,
            width,
            self.rows.len() + 3,
            40,
            menu.bounds,
            menu.list_area,
        )
    }

    pub(super) fn draw(self, frame: &mut Frame, area: Rect, menu: &FlowMenuDraw) -> (Vec<(Rect, usize)>, usize) {
        let inner = preview_frame(frame, area, menu.steps.is_some());

        frame.render_widget(
            Paragraph::new(self.head),
            Rect {
                height: 1.min(inner.height),
                ..inner
            },
        );

        if !self.flow.description.is_empty() && inner.height > 1 {
            frame.render_widget(
                Paragraph::new(Line::styled(
                    crate::ui::thread_card::fit(&self.flow.description, usize::from(inner.width)),
                    theme::muted(),
                )),
                Rect {
                    y: inner.y + 1,
                    height: 1,
                    ..inner
                },
            );
        }

        let rail = Rect {
            y: inner.y + 3,
            height: inner.height.saturating_sub(3),
            ..inner
        };

        let cursor = match menu.steps {
            Some((step, _)) => Some(step),
            None => (menu.cursor == menu.selected_flow).then_some(menu.selected_step),
        };

        let offset = menu.steps.map_or(0, |(step, offset)| {
            dropdown::fit(offset, step, self.rows.len(), usize::from(rail.height))
        });

        let rows = flow_rail::draw(
            frame.buffer_mut(),
            rail,
            &self.rows,
            &self.arcs,
            cursor,
            menu.steps.is_some(),
            offset,
        );

        (rows, offset)
    }
}

fn preview_frame(frame: &mut Frame, area: Rect, active: bool) -> Rect {
    frame.render_widget(Clear, area);

    let note = if active {
        " ↑↓ start · enter: pick · ← back "
    } else {
        " → start step · enter: from step 1 "
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(if active { theme::accent() } else { theme::line() })
        .title(Span::styled(" preview ", theme::muted()))
        .title_bottom(Span::styled(note, theme::muted()));
    let inner = block.inner(area);

    frame.render_widget(block, area);

    Rect {
        x: inner.x + 1,
        width: inner.width.saturating_sub(2),
        ..inner
    }
}

fn titles_width(rows: &[Row]) -> usize {
    rows.iter()
        .map(|row| row.title.width() + row.tags.iter().map(|(title, _)| title.width() + 2).sum::<usize>())
        .max()
        .unwrap_or(0)
}

fn labels_width(nodes: &[Resolved]) -> usize {
    nodes
        .iter()
        .flat_map(|node| node.edges.iter().map(|(key, _)| key.width() + 4))
        .chain(
            nodes
                .iter()
                .map(|node| node.loops.as_ref().map_or(0, |label| label.1.width() + 14)),
        )
        .max()
        .unwrap_or(0)
}
