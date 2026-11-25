use super::{INPUT_ROWS, MIN_HEADER_SCREEN, Pane, Rect, View, header_rows, input_width};

pub(super) struct PromptLayout {
    pub(super) area: Rect,
    pub(super) footer: Rect,
    pub(super) hint: Rect,
    pub(super) input: Rect,
    pub(super) bubble: Rect,
    pub(super) header: Rect,
    pub(super) search: Rect,
    pub(super) title: Rect,
}

impl PromptLayout {
    pub(super) fn new(view: &View, area: Rect) -> Self {
        let footer = Rect {
            y: area.bottom().saturating_sub(1),
            height: 1,
            ..area
        };
        let hint = Rect {
            y: footer.y.saturating_sub(1),
            ..footer
        };
        let width = input_width(area.width).max(1);
        let rows = (view.prompt.input.rows_for(width) as u16).clamp(1, INPUT_ROWS);
        let bottom = hint.y.saturating_sub(1);
        let input_top = bottom.saturating_sub(rows);
        let input = Rect {
            x: area.x + 2,
            y: input_top,
            width,
            height: rows,
        };
        let bubble_top = input_top.saturating_sub(1);
        let bubble = Rect {
            y: bubble_top,
            height: bottom + 1 - bubble_top,
            ..area
        };
        let header_height = Self::header_height(view, area);
        let search_height = u16::from(view.find.open && (view.panes.pane == Pane::Chat || view.layout.wide_split));
        let search = Rect {
            x: area.x + 1,
            y: bubble_top.saturating_sub(search_height),
            width: area.width.saturating_sub(2),
            height: search_height,
        };
        let header = Rect {
            y: (bubble_top + 1).saturating_sub(header_height + 2 * search_height),
            height: header_height,
            ..area
        };
        let title = Rect {
            y: if header_height > 0 { header.y } else { search.y }.saturating_sub(1),
            height: 1,
            ..area
        };

        Self {
            area,
            footer,
            hint,
            input,
            bubble,
            header,
            search,
            title,
        }
    }

    fn header_height(view: &View, area: Rect) -> u16 {
        if area.height >= MIN_HEADER_SCREEN
            && !view.agents.members.is_empty()
            && (view.panes.pane == Pane::Chat || view.layout.wide_split)
        {
            header_rows(view.agents.members.len()) + 1
        } else {
            0
        }
    }
}

pub(super) struct ActivityLayout {
    pub(super) flow_rows: Vec<u16>,
    pub(super) flow: Rect,
    pub(super) monitors: Rect,
    pub(super) subagents: Rect,
    pub(super) subthreads: Rect,
    pub(super) stream: Rect,
}

impl ActivityLayout {
    pub(super) fn new(view: &mut View, prompt: &PromptLayout) -> Self {
        let area = prompt.area;
        let flow_rows: Vec<u16> = view.flowbars.iter().map(|flow| flow.rows(area.height / 2)).collect();
        let flow_height = flow_rows.iter().sum::<u16>().min(area.height / 2);
        let flow = Rect {
            y: prompt.title.y.saturating_sub(flow_height),
            height: flow_height,
            ..area
        };
        let monitor_height = view.monitors.rows(area.width, area.height / 3);
        let monitors = Rect {
            y: flow.y.saturating_sub(monitor_height),
            height: monitor_height,
            ..area
        };
        let subagent_height = view.subagents.rows(&view.thread.subs, area.width, area.height / 3);
        let subagents = Rect {
            y: monitors.y.saturating_sub(subagent_height),
            height: subagent_height,
            ..area
        };
        let subthread_height = view.subthreads.rows(area.width, area.height / 3);
        let subthreads = Rect {
            y: subagents.y.saturating_sub(subthread_height),
            height: subthread_height,
            ..area
        };
        let stream = Rect {
            height: subthreads.y.saturating_sub(area.y),
            ..area
        };

        Self {
            flow_rows,
            flow,
            monitors,
            subagents,
            subthreads,
            stream,
        }
    }
}
