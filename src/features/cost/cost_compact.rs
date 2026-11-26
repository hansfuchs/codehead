use super::{AgentRow, Limit, Line, Snap, Span, Style, agents, dither_bar, theme, until, window, window_bar};

/// Width of a window's tail: ` 55% 3h`.
const WINDOW_TAIL: usize = 8;
/// Width of a context's tail: `  26%`.
const CONTEXT_TAIL: usize = 5;

/// The agents of 1 harness and the plan windows of its account.
pub(super) struct Group<'a> {
    pub harness: &'a str,
    pub agents: Vec<&'a AgentRow>,
    pub limits: Vec<&'a Limit>,
}

/// The snapshot's agents by harness (in the agents' order: the costliest harness 1st).
pub(super) fn groups(snapshot: &Snap) -> Vec<Group<'_>> {
    let mut groups: Vec<Group> = Vec::new();

    for agent in &snapshot.agents {
        let harness = agent.usage.harness.as_str();

        match groups.iter_mut().find(|group| group.harness == harness) {
            Some(group) => group.agents.push(agent),
            None => groups.push(Group {
                harness,
                agents: vec![agent],
                limits: snapshot
                    .limits
                    .iter()
                    .filter(|limit| limit.harness == harness)
                    .collect(),
            }),
        }
    }

    groups
}

/// The compact box, WIDTH columns: per harness 1 row with its plan windows, then 1 row per agent
/// with its context. Every bar starts in 1 column. `cost_next`: the next message (its share of
/// each window).
pub(super) fn compact_lines(
    snapshot: &Snap,
    width: usize,
    now: u64,
    cost_next: Option<(u64, f64)>,
) -> Vec<Line<'static>> {
    let groups = groups(snapshot);
    let label_width = groups
        .iter()
        .flat_map(|group| {
            std::iter::once(group.harness.chars().count())
                .chain(group.agents.iter().map(|agent| agent.name.chars().count()))
        })
        .max()
        .unwrap_or(0)
        + 1;
    let tag_width = snapshot
        .limits
        .iter()
        .map(|limit| window(&limit.label).chars().count() + 1)
        .max()
        .unwrap_or(0);
    let grid = Grid {
        width,
        label_width,
        tag_width,
        now,
        cost_next,
    };
    let mut lines = Vec::new();

    for group in &groups {
        lines.push(grid.harness_line(group));

        for agent in &group.agents {
            lines.push(grid.agent_line(agent));
        }
    }

    lines
}

/// The column grid of the compact box: label, window tag, bar, tail.
struct Grid {
    width: usize,
    label_width: usize,
    tag_width: usize,
    now: u64,
    cost_next: Option<(u64, f64)>,
}

impl Grid {
    /// The harness, its windows side by side (1 segment each, 2 columns between).
    fn harness_line(&self, group: &Group) -> Line<'static> {
        let mut spans = vec![Span::styled(
            format!("{:width$}", group.harness, width = self.label_width),
            theme::muted(),
        )];
        let count = group.limits.len();

        if count == 0 {
            spans.push(Span::styled("0 plan windows", theme::line()));
            return Line::from(spans);
        }

        let room = self.width.saturating_sub(self.label_width + 2 * (count - 1));
        let segment = room / count;

        for (i, limit) in group.limits.iter().enumerate() {
            spans.extend((i > 0).then(|| Span::raw("  ")));
            spans.extend(self.window_segment(limit, segment));
        }

        Line::from(spans)
    }

    /// 1 window in SEGMENT columns: its tag, its bar, its % and reset.
    fn window_segment(&self, limit: &Limit, segment: usize) -> Vec<Span<'static>> {
        // window reset since the last report: 0 %
        let fresh = limit.resets.is_some_and(|at| at <= self.now);
        let pct = if fresh { 0.0 } else { limit.pct };

        let tail = match limit.resets {
            _ if fresh => " new".to_string(),
            Some(at) => format!(" {:>3.0}% {:>2}", pct, until(at, self.now)),
            None => format!(" {pct:>3.0}%"),
        };

        let bar_width = segment.saturating_sub(self.tag_width + WINDOW_TAIL).max(1);
        let mut spans = vec![Span::styled(
            format!("{:width$}", window(&limit.label), width = self.tag_width),
            theme::muted(),
        )];

        spans.extend(window_bar(
            pct,
            super::render::share_of(limit, self.cost_next),
            bar_width,
        ));
        let tail_width = WINDOW_TAIL;

        spans.push(Span::styled(format!("{tail:<tail_width$}"), theme::muted()));

        spans
    }

    /// The agent (bold: live), its context bar in its color (faded: 0 live session, error: ≥ 90 %).
    fn agent_line(&self, agent: &AgentRow) -> Line<'static> {
        let color = agents::color(agent.color.as_deref());
        let name = Style::default().fg(color);

        let name = if agent.live {
            name.add_modifier(ratatui::style::Modifier::BOLD)
        } else {
            name
        };

        let pct = agent.ctx.unwrap_or(0) as f64;

        let fill = if pct >= 90.0 {
            theme::pal().error()
        } else if agent.live {
            color
        } else {
            crate::domain::palette::tint(color, theme::pal().base, 0.45)
        };

        let bar_width = self
            .width
            .saturating_sub(self.label_width + self.tag_width + CONTEXT_TAIL)
            .max(1);
        let mut spans = vec![
            Span::styled(format!("{:width$}", agent.name, width = self.label_width), name),
            Span::raw(" ".repeat(self.tag_width)),
        ];

        spans.extend(dither_bar(pct, 0.0, bar_width, fill));
        spans.push(Span::styled(
            format!(" {:>4}", agent.ctx.map_or("·".into(), |pct| format!("{pct}%"))),
            theme::muted(),
        ));

        Line::from(spans)
    }
}
