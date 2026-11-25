use super::Color;
use std::ops::ControlFlow;

use super::{Card, Frame, Modifier, Paragraph, Rect, Span, Style, View, WAVE, cards_w, chat, theme, title_case};

impl View {
    /// Header card of agent NAME (hell's): sprite, color, the lines right of the sprite (name,
    /// `Claude · Opus 5.5`, effort + status), width incl. 4 columns gap to the next card.
    pub(super) fn card(&self, name: &str) -> Card {
        let color = self.color(name);
        let (sprite, effort) = self
            .agents
            .profiles
            .get(name)
            .cloned()
            .unwrap_or_else(|| (crate::domain::sprites::default_for(name).desynced(name), String::new()));
        let tech = card_tech(self, name);
        let status = card_status(self, name, color);
        let status_width = if status.is_empty() { 0 } else { 17 };
        let mut text_width = [
            name.chars().count(),
            tech.chars().count(),
            effort.chars().count() + status_width,
        ]
        .into_iter()
        .max()
        .unwrap_or(0);
        let flow = card_flow(self, name, &mut text_width);
        let mut last = vec![Span::styled(format!("  {effort}"), theme::muted())];

        last.extend(status);
        let mut side = vec![
            vec![],
            card_name(self, name, color),
            vec![Span::styled(format!("  {tech}"), theme::muted())],
            last,
        ];

        if let Some(flow) = flow {
            side.remove(0);
            side.push(flow);
        }

        let side = decode_card(self, name, color, side);

        Card {
            sprite,
            color,
            side,
            width: (crate::ui::sprites::WIDTH + 2 + text_width + 4) as u16,
        }
    }

    /// Header (hell's): 1 frame over the agents box (the cards) and the tokens box (COST:
    /// the pane's area, 0 width: 0 tokens box), straight lines between them.
    pub(super) fn draw_header(&self, frame: &mut Frame, area: Rect, cost: Rect) {
        if area.height == 0 {
            self.agents.slots.borrow_mut().clear();
            self.agents.flow_slots.borrow_mut().clear();
            return;
        }

        let cards: Vec<Card> = self.agents.order.iter().map(|name| self.card(name)).collect();
        let token_left = cost.x.saturating_sub(2);
        let fit = if cost.width > 0 { token_left + 1 } else { area.right() }.saturating_sub(area.x);

        let outer = if cost.width == 0 {
            area
        } else {
            Rect {
                width: cards_w(&cards).min(fit),
                ..area
            }
        };

        let card_height = crate::ui::sprites::HEIGHT as u16;
        let y = (area.y + 1).max(area.bottom().saturating_sub(1 + card_height));
        let rows = Rect {
            y,
            height: area.bottom().saturating_sub(1).saturating_sub(y),
            ..area
        };
        let mut layout = HeaderLayout {
            outer,
            rows,
            x: area.x + 2,
            slots: Vec::new(),
            flow_slots: Vec::new(),
        };
        let _ = cards
            .into_iter()
            .zip(&self.agents.order)
            .try_for_each(|(card, name)| layout.draw(frame, self, card, name));

        *self.agents.slots.borrow_mut() = layout.slots;
        *self.agents.flow_slots.borrow_mut() = layout.flow_slots;
    }

    /// Who gets a message now: the @named agents of the input, else the lead.
    pub(super) fn recipients(&self) -> Vec<String> {
        let text = self.prompt.input.text();
        let named = crate::engine::host::mentions(&crate::ui::mention::strip_replaces(&text), &self.agents.members);

        if named.is_empty() { self.default_to() } else { named }
    }

    /// Who a message without @name goes to: the agents of the owner's last message (still on the
    /// thread), else the lead.
    pub(super) fn default_to(&self) -> Vec<String> {
        let last = self.thread.items.iter().rev().find_map(|item| match item {
            chat::Item::Prompt { from, to, .. } if from == "owner" => Some(
                to.iter()
                    .filter(|agent| self.agents.members.contains(agent))
                    .cloned()
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        });

        match last {
            Some(to) if !to.is_empty() => to,
            _ => self.agents.members.first().cloned().into_iter().collect(),
        }
    }
}

struct HeaderLayout {
    outer: Rect,
    rows: Rect,
    x: u16,
    slots: Vec<(String, Rect)>,
    flow_slots: Vec<(String, Rect)>,
}
impl HeaderLayout {
    fn draw(&mut self, frame: &mut Frame, view: &View, card: Card, name: &str) -> ControlFlow<()> {
        if self.x + card.width - 1 > self.outer.right() {
            return ControlFlow::Break(());
        }

        let status = view.avatar_status(name);

        frame.render_widget(
            Paragraph::new(crate::ui::sprites::lines_as(
                &card.sprite,
                card.color,
                status,
                card.side,
            )),
            Rect {
                x: self.x,
                width: card.width - 4,
                ..self.rows
            },
        );
        self.slots.push((
            name.to_string(),
            Rect {
                x: self.x,
                y: self.rows.y,
                width: crate::ui::sprites::WIDTH as u16,
                height: crate::ui::sprites::HEIGHT as u16,
            },
        ));

        if view
            .flowbars
            .iter()
            .any(|flow| flow.agent == name && flow.state.is_some())
        {
            self.flow_slot(name, card.width);
        }

        self.x += card.width;

        ControlFlow::Continue(())
    }
    fn flow_slot(&mut self, name: &str, width: u16) {
        let sprite_width = crate::ui::sprites::WIDTH as u16;

        self.flow_slots.push((
            name.to_string(),
            Rect {
                x: self.x + sprite_width,
                y: self.rows.y + 3,
                width: (width - 4).saturating_sub(sprite_width),
                height: 1,
            }
            .intersection(self.rows),
        ));
    }
}
fn decode_card_line(line: Vec<Span<'static>>, reveal: f32, noise: Style, dim: Style) -> Vec<Span<'static>> {
    if reveal <= 0.0 {
        Vec::new()
    } else {
        crate::ui::avatar::decode(line, reveal, noise, dim)
    }
}

fn card_tech(view: &View, name: &str) -> String {
    let agent = view.agents.roster.iter().find(|agent| agent.name == name);
    let model = view
        .thread
        .models
        .get(name)
        .filter(|model| !model.is_empty())
        .map(|model| crate::domain::agents::model_name(model))
        .or_else(|| agent.map(crate::domain::agents::model_of))
        .unwrap_or_default();
    let harness = agent.map(|agent| title_case(&agent.harness)).unwrap_or_default();

    [harness, model]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

fn card_status(view: &View, name: &str, color: Color) -> Vec<Span<'static>> {
    let wave = WAVE[view.tick() % WAVE.len()];

    if view.thread.asks.iter().any(|ask| ask.agent == name) {
        return vec![
            Span::styled("  ● ", theme::fg(theme::pal().attention())),
            Span::styled("waits on you", theme::muted()),
        ];
    }

    let label = if view.thread.busy.contains(name) {
        "working…"
    } else if view.thread.working(name) {
        "subagents…"
    } else if view.monitors.watches.iter().any(|watch| watch.agent == name) {
        "monitoring…"
    } else {
        return Vec::new();
    };

    vec![
        Span::styled(format!("  {wave} "), Style::default().fg(color)),
        Span::styled(label, theme::muted()),
    ]
}

fn card_name(view: &View, name: &str, color: Color) -> Vec<Span<'static>> {
    let focused = view
        .agents
        .agents_focus
        .as_ref()
        .is_some_and(|focus| view.agents.order.get(focus.at).is_some_and(|agent| agent == name));

    if focused {
        vec![
            Span::raw(" "),
            Span::styled(format!(" {name} "), theme::pill().add_modifier(Modifier::BOLD)),
        ]
    } else {
        vec![Span::styled(
            format!("  {name}"),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )]
    }
}

fn card_flow(view: &View, name: &str, width: &mut usize) -> Option<Vec<Span<'static>>> {
    let (text, style) = view
        .flowbars
        .iter()
        .find(|flow| flow.agent == name)
        .and_then(crate::features::flows::flow_bar::FlowBar::card_line)?;
    let text = crate::ui::thread_card::fit(&text, (*width).max(34));

    *width = (*width).max(text.chars().count());
    Some(vec![Span::styled(format!("  {text}"), style)])
}

fn decode_card(view: &View, name: &str, color: Color, side: Vec<Vec<Span<'static>>>) -> Vec<Vec<Span<'static>>> {
    let Some(spawn) = view.agents.spawns.get(name) else {
        return side;
    };

    let reveal = spawn.reveal();
    let noise = Style::default().fg(color);
    let dim = theme::line();

    side.into_iter()
        .map(|line| decode_card_line(line, reveal, noise, dim))
        .collect()
}
