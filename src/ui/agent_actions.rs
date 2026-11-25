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

    for usage in uses {
        leave_thread(agent, fallback, &mut out, danger, usage);
    }

    let archived = roster::get(&agent.name)
        .ok_or_else(|| format!("no agent {}", agent.name))
        .and_then(|mut record| {
            record.archived = true;
            roster::save(&record)
        });

    match archived {
        Ok(()) => out.push(Line::styled(
            format!("{} deleted (archived: file + history kept)", agent.name),
            Style::default().fg(theme::pal().success()),
        )),
        Err(error) => out.push(Line::styled(format!("archive failed: {error}"), danger)),
    }

    out
}

fn leave_thread(agent: &Agent, fallback: &Agent, out: &mut Vec<Line<'_>>, danger: Style, usage: &Use) {
    let short: String = usage.thread_id.chars().take(12).collect();
    let mut line = vec![
        Span::styled(format!("  {short} "), theme::muted()),
        Span::raw(usage.title.clone()),
    ];

    let left = if client::alive(&usage.thread_id) {
        client::send(
            &usage.thread_id,
            &serde_json::json!({"op": "leave", "agent": agent.name}),
        )
        .map(|_| ())
    } else {
        // as the host's leave: its flow goes first (the lead's old flow field too)
        store::update(&usage.thread_id, |thread| {
            thread.set_flow(&agent.name, None);
            thread.agents.retain(|name| *name != agent.name);
        })
        .map(|_| ())
    };

    if let Err(error) = left {
        line.push(Span::styled(format!(" · leave failed: {error}"), danger));
    }

    if store::load(&usage.thread_id).is_some_and(|thread| thread.agents.is_empty()) {
        match store::update(&usage.thread_id, |thread| thread.agents.push(fallback.name.clone())) {
            Ok(_) => line.push(Span::styled(
                format!(" · {} leads now", fallback.name),
                Style::default().fg(theme::pal().success()),
            )),
            Err(error) => line.push(Span::styled(format!(" · {error}"), danger)),
        }
    }

    out.push(Line::from(line));
}
