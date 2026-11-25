use super::{FlowBar, FlowState, Modifier, Resolved, Span, Style, Wait, theme};
use std::fmt::Write as FormatWrite;

impl FlowBar {
    /// The agent card's flow line: `◆ 4/8 implement` (a gate wait: ⏸, finished: ✓,
    /// warnings: `!2`) and its style. None: 0 flow.
    pub(crate) fn card_line(&self) -> Option<(String, Style)> {
        card_line_flow_bar(self)
    }
    /// The row's text and style.
    pub(super) fn bar_line(&self) -> (Vec<Span<'static>>, Style) {
        bar_line_flow_bar(self)
    }
}
pub(super) fn push_round(run: &FlowState, text: &mut String, node: &Resolved, max: u32) {
    let round = run.round(&node.id);

    if round > 0 {
        write!(text, " · round {}/{max}", round + 1).unwrap();
    }
}
pub(super) fn push_wait_and_switches(
    run: &FlowState,
    mut spans: Vec<Span<'static>>,
    style: Style,
    step: &str,
    bar: &FlowBar,
) -> (Vec<Span<'static>>, Style) {
    match &run.wait {
        Some(Wait::Gate) => spans.push(Span::styled(
            format!("  ⏸ gate {step}: answer below"),
            theme::fg(theme::pal().attention()).add_modifier(Modifier::BOLD),
        )),
        Some(Wait::LoopMax) => spans.push(Span::styled(
            "  ⏸ loop max reached: answer below".to_string(),
            theme::fg(theme::pal().attention()).add_modifier(Modifier::BOLD),
        )),
        None => {}
    }

    if let Some(switch) = &run.pending {
        let name = bar
            .lib
            .flow(&switch.flow)
            .map_or(switch.flow.clone(), |flow| flow.name.clone());

        spans.push(Span::styled(
            format!("  → {name} at the next tool boundary"),
            theme::fg(theme::pal().progress()),
        ));
    }

    if let Some(switch) = &run.queued {
        let name = bar
            .lib
            .flow(&switch.flow)
            .map_or(switch.flow.clone(), |flow| flow.name.clone());

        spans.push(Span::styled(format!("  → queued: {name}"), theme::muted()));
    }

    if !bar.warnings.is_empty() {
        spans.push(Span::styled(
            format!("  ! {}", bar.warnings.len()),
            Style::default().fg(theme::pal().error()),
        ));
    }

    (spans, style)
}
pub(super) fn push_step_text(run: &FlowState, spans: &mut Vec<Span<'_>>, total: usize, step: &String, bar: &FlowBar) {
    let mut text = format!("◆ {} · {}/{total} {step}", bar.name, run.node + 1);

    if let Some(node) = bar.nodes.get(run.node)
        && let Some((_, _, max)) = &node.loops
    {
        push_round(run, &mut text, node, *max);
    }

    write!(text, " · @{}", bar.agent).unwrap();
    spans.push(Span::raw(text));
}

fn card_line_flow_bar(bar: &FlowBar) -> Option<(String, Style)> {
    let run = bar.state.as_ref()?;

    if run.finished {
        return Some((
            format!("✓ {} finished", bar.name),
            Style::default().fg(theme::pal().success()),
        ));
    }

    let step = bar
        .nodes
        .get(run.node)
        .map(|node| node.title.clone())
        .unwrap_or_default();
    let mut text = format!(
        "{} {}/{} {step}",
        if run.wait.is_some() { "⏸" } else { "◆" },
        run.node + 1,
        bar.nodes.len()
    );

    if !bar.warnings.is_empty() {
        write!(text, " !{}", bar.warnings.len()).unwrap();
    }

    // a switch at the next tool boundary
    if let Some(switch) = &run.pending {
        write!(
            text,
            " → {}",
            bar.lib
                .flow(&switch.flow)
                .map_or(switch.flow.clone(), |flow| flow.name.clone())
        )
        .unwrap();
    }

    let style = if run.wait.is_some() {
        theme::fg(theme::pal().attention())
    } else {
        theme::fg(theme::pal().progress())
    };

    Some((text, style))
}

fn bar_line_flow_bar(bar: &FlowBar) -> (Vec<Span<'static>>, Style) {
    let Some(run) = &bar.state else {
        return (
            vec![Span::raw(format!("◆ @{} · 0 flow: pick one", bar.agent))],
            theme::muted(),
        );
    };

    let mut spans = Vec::new();
    let mut style = theme::accent();
    let total = bar.nodes.len();
    let step = bar
        .nodes
        .get(run.node)
        .map(|node| node.title.clone())
        .unwrap_or_default();

    if run.finished {
        spans.push(Span::raw(format!("◆ {} · finished", bar.name)));
        style = Style::default().fg(theme::pal().success());
    } else {
        push_step_text(run, &mut spans, total, &step, bar);
    }

    push_wait_and_switches(run, spans, style, &step, bar)
}
