use crate::domain::agents::{Agent, Use, color};
use crate::engine::{client, roster, store};
use crate::ui::theme;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use std::path::Path;
/// Archive AGENT. It leaves its threads; a thread left with 0 agents gets FALLBACK as lead.
pub(crate) fn delete(agent: &Agent, fallback: &Agent, uses: &[Use], _logs: &Path) -> Vec<Line<'static>> {
    let mut out = vec![Line::from(vec![
        Span::styled(agent.name.clone(), Style::default().fg(color(agent.color.as_deref()))),
        Span::styled(format!("  {} threads", uses.len()), theme::muted()),
    ])];
    let danger = Style::default().fg(theme::pal().error());

    for user in uses {
        delete_short(agent, fallback, &mut out, danger, user);
    }

    let archived = roster::get(&agent.name)
        .ok_or_else(|| format!("no agent {}", agent.name))
        .and_then(|mut result| {
            result.archived = true;
            roster::save(&result)
        });

    match archived {
        Ok(()) => out.push(Line::styled(
            format!("{} deleted (archived: file + history kept)", agent.name),
            Style::default().fg(theme::pal().success()),
        )),
        Err(error_3) => out.push(Line::styled(format!("archive failed: {error_3}"), danger)),
    }

    out
}

fn delete_short(agent: &Agent, fallback: &Agent, out: &mut Vec<Line<'_>>, danger: Style, user: &Use) {
    let short: String = user.thread_id.chars().take(12).collect();
    let mut line = vec![
        Span::styled(format!("  {short} "), theme::muted()),
        Span::raw(user.title.clone()),
    ];

    let left = if client::alive(&user.thread_id) {
        client::send(
            &user.thread_id,
            &serde_json::json!({"op": "leave", "agent": agent.name}),
        )
        .map(|_| ())
    } else {
        // as the host's leave: its flow goes first (the lead's old flow field too)
        store::update(&user.thread_id, |thread| {
            thread.set_flow(&agent.name, None);
            thread.agents.retain(|agent_value| *agent_value != agent.name);
        })
        .map(|_| ())
    };

    if let Err(error_value) = left {
        line.push(Span::styled(format!(" · leave failed: {error_value}"), danger));
    }

    if store::load(&user.thread_id).is_some_and(|thread_value| thread_value.agents.is_empty()) {
        match store::update(&user.thread_id, |thread_2| thread_2.agents.push(fallback.name.clone())) {
            Ok(_) => line.push(Span::styled(
                format!(" · {} leads now", fallback.name),
                Style::default().fg(theme::pal().success()),
            )),
            Err(error_2) => line.push(Span::styled(format!(" · {error_2}"), danger)),
        }
    }

    out.push(Line::from(line));
}
