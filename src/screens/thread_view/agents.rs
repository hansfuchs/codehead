use super::Rect;

use super::{Color, Frame, HashSet, View, chat, effort_label, store, theme};

impl View {
    /// The changes view's agents (the comment modal's `@` dropdown, its mentions).
    pub(super) fn changes_agents(&mut self) {
        self.panes.changes.agents = self.agents.roster.clone();
        self.panes.changes.members = self.agents.members.clone();
    }

    /// The agents (each 5 s, join, leave): the stream redraws only when its inputs changed.
    pub(super) fn set_roster(&mut self, roster: Vec<crate::domain::agents::Agent>) {
        let colors: chat::Colors = roster
            .iter()
            .map(|agent| (agent.name.clone(), crate::domain::agents::color(agent.color.as_deref())))
            .collect();
        let harness_of: Vec<(String, String)> = roster
            .iter()
            .map(|agent| (agent.name.clone(), agent.harness.clone()))
            .collect();

        if colors != self.agents.colors || harness_of != self.agents.harness_of {
            self.stream.dirty = true;
        }

        self.agents.colors = colors;
        self.agents.harness_of = harness_of;
        self.agents.names = roster
            .iter()
            .map(|agent| (agent.name.clone(), crate::domain::agents::color(agent.color.as_deref())))
            .collect();
        self.prompt.input.marks = self.agents.names.clone();
        let global = crate::domain::agents::effort_name();

        self.agents.profiles = roster
            .iter()
            .map(|agent| {
                let effort = crate::engine::roster::get(&agent.name)
                    .and_then(|result| result.effort)
                    .or_else(|| (agent.harness == "claude").then(|| global.clone()))
                    .filter(|error| error != "default")
                    .map(|error| effort_label(&error))
                    .unwrap_or_default();

                (
                    agent.name.clone(),
                    (crate::domain::agents::sprite(agent).desynced(&agent.name), effort),
                )
            })
            .collect();

        self.agents.roster = roster;
        self.agents.members = store::load(&self.id).map(|thread| thread.agents).unwrap_or_default();
        self.prompt.mention.replaceable = self.agents.members.clone();
        self.changes_agents();
        self.sync_order();
        self.sync_flowbars();
    }

    /// The box order after a change of the members: gone ones out, new ones in (at the front
    /// when @named, else at the end), each new one spawns. The 1st load: all spawn (the thread
    /// opens).
    pub(super) fn sync_order(&mut self) {
        let first = self.agents.order.is_empty();

        self.agents.order.retain(|count| self.agents.members.contains(count));

        for count in self.agents.members.clone() {
            sync_order_branch(self, count);
        }

        self.agents.front.retain(|count| !self.agents.members.contains(count));

        if first {
            self.stream.dirty = true;
        }
    }

    /// NAMED were addressed: to the front of the box; one out of view spawns.
    pub(super) fn bring_front(&mut self, named: &[String]) {
        let shown: HashSet<String> = self
            .agents
            .slots
            .borrow()
            .iter()
            .map(|(count, _)| count.clone())
            .collect();

        for count in named.iter().rev() {
            bring_agent_front(self, count, &shown);
        }
    }

    /// A spawn for 1 agent: an animation no other agent of the thread plays now.
    pub(super) fn new_spawn(&self) -> crate::domain::avatar::Spawn {
        let taken: Vec<usize> = self
            .agents
            .spawns
            .values()
            .filter(|text| !text.done())
            .map(|text| text.anim)
            .collect();

        crate::domain::avatar::Spawn::random_except(&taken)
    }

    /// A spawn (or its text decode) runs: frames at 30 per second.
    pub(super) fn spawning(&self) -> bool {
        self.agents.spawns.values().any(|text| !text.over()) || crate::features::cost::morphing()
    }

    /// The color of agent NAME now: blocked (a question waits on the owner): all warn yellow,
    /// else its own.
    pub(super) fn color(&self, name: &str) -> Color {
        if self.thread.asks.iter().any(|query| query.agent == name) {
            theme::pal().attention()
        } else {
            self.agents.colors.get(name).copied().unwrap_or(theme::pal().accent)
        }
    }

    /// The agent colors now (blocked agents: yellow).
    pub(super) fn colors_now(&self) -> chat::Colors {
        let mut character = self.agents.colors.clone();

        for query in &self.thread.asks {
            character.insert(query.agent.clone(), theme::pal().attention());
        }

        character
    }

    /// The avatar status of agent NAME.
    pub(super) fn avatar_status(&self, name: &str) -> crate::domain::avatar::Status {
        use crate::domain::avatar::Status;

        if self.thread.asks.iter().any(|query| query.agent == name) {
            Status::Blocked
        } else if self.thread.busy.contains(name) || self.thread.working(name) {
            Status::Working
        } else if self.monitors.watches.iter().any(|width| width.agent == name) {
            Status::Thinking
        } else {
            Status::Idle
        }
    }

    /// The spawns over the header (and what is below it), in the agent colors.
    pub(super) fn draw_spawns(&mut self, frame: &mut Frame) {
        self.agents.spawns.retain(|_, text| !text.over());
        let area = frame.area();

        for (name, slot) in self.agents.slots.borrow().iter() {
            draw_spawns_sprite(self, frame, area, name, *slot);
        }
    }
}

struct SpawnGeometry {
    area: Rect,
    slot: Rect,
    origin: (i32, i32),
    color: Color,
}
fn draw_spawn_row(buffer: &mut ratatui::buffer::Buffer, geometry: &SpawnGeometry, index: usize, row: &str) {
    let SpawnGeometry {
        area,
        slot,
        origin: (x0, y0),
        color,
    } = *geometry;
    let bottom = i32::from(slot.y + slot.height);

    for (column, character) in row.chars().enumerate() {
        let (x, y) = (x0 + column as i32, y0 + index as i32);

        if x < i32::from(area.x)
            || y < i32::from(area.y)
            || x >= i32::from(area.right())
            || y >= i32::from(area.bottom()).min(bottom)
        {
            continue;
        }

        let inside = (i32::from(slot.x)..i32::from(slot.x + slot.width)).contains(&x)
            && (i32::from(slot.y)..bottom).contains(&y);

        if !inside && (character == ' ' || character == '\u{2800}') {
            continue;
        }

        buffer[(x as u16, y as u16)]
            .set_char(character)
            .set_style(theme::text().fg(color));
    }
}

fn sync_order_branch(view: &mut View, count: String) {
    if view.agents.order.contains(&count) {
        return;
    }

    if view.agents.front.contains(&count) {
        view.agents.order.insert(0, count.clone());
    } else {
        view.agents.order.push(count.clone());
    }

    let sp = view.new_spawn();

    view.agents.spawns.insert(count, sp);
}

fn bring_agent_front(view: &mut View, name: &str, shown: &HashSet<String>) {
    let Some(index) = view.agents.order.iter().position(|agent| agent == name) else {
        remember_front(view, name);
        return;
    };

    let agent = view.agents.order.remove(index);

    if !shown.contains(&agent) {
        let spawn = view.new_spawn();

        view.agents.spawns.insert(agent.clone(), spawn);
    }

    view.agents.order.insert(0, agent);
}

fn remember_front(view: &mut View, name: &str) {
    if !view.agents.front.iter().any(|agent| agent == name) {
        view.agents.front.push(name.to_string());
    }
}

fn draw_spawns_sprite(
    view: &View,
    frame: &mut Frame<'_>,
    area: ratatui::prelude::Rect,
    name: &String,
    slot: ratatui::prelude::Rect,
) {
    let Some(sp) = view.agents.spawns.get(name) else {
        return;
    };

    let sprite = view.agents.profiles.get(name).map_or_else(
        || crate::domain::sprites::default_for(name).desynced(name),
        |path| path.0.clone(),
    );

    let Some(rows) = sp.rows(&sprite.avatar, view.avatar_status(name)) else {
        return;
    };

    let color = view.color(name);
    let (ax, ay) = crate::domain::avatar::SPAWN_AT;
    let (x0, y0) = (i32::from(slot.x) - ax as i32, i32::from(slot.y) - ay as i32);
    let geometry = SpawnGeometry {
        area,
        slot,
        origin: (x0, y0),
        color,
    };
    let buffer = frame.buffer_mut();

    for (index, row) in rows.iter().enumerate() {
        draw_spawn_row(buffer, &geometry, index, row);
    }
}
