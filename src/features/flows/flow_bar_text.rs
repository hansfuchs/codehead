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
pub(super) fn bar_line_text_result(st: &FlowState, text: &mut String, count_value: &Resolved, max: u32) {
    let result = st.round(&count_value.id);

    if result > 0 {
        write!(text, " · round {}/{max}", result + 1).unwrap();
    }
}
pub(super) fn finish_bar_line(
    st: &FlowState,
    mut spans: Vec<Span<'static>>,
    style: Style,
    step: &str,
    view_state: &FlowBar,
) -> (Vec<Span<'static>>, Style) {
    match &st.wait {
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

    if let Some(path) = &st.pending {
        let name = view_state
            .lib
            .flow(&path.flow)
            .map_or(path.flow.clone(), |file| file.name.clone());

        spans.push(Span::styled(
            format!("  → {name} at the next tool boundary"),
            theme::fg(theme::pal().progress()),
        ));
    }

    if let Some(query) = &st.queued {
        let name = view_state
            .lib
            .flow(&query.flow)
            .map_or(query.flow.clone(), |file_value| file_value.name.clone());

        spans.push(Span::styled(format!("  → queued: {name}"), theme::muted()));
    }

    if !view_state.warnings.is_empty() {
        spans.push(Span::styled(
            format!("  ! {}", view_state.warnings.len()),
            Style::default().fg(theme::pal().error()),
        ));
    }

    (spans, style)
}
pub(super) fn bar_line_text(
    st: &FlowState,
    spans: &mut Vec<Span<'_>>,
    total: usize,
    step: &String,
    view_state: &FlowBar,
) {
    let mut text = format!("◆ {} · {}/{total} {step}", view_state.name, st.node + 1);

    if let Some(count_value) = view_state.nodes.get(st.node)
        && let Some((_, _, max)) = &count_value.loops
    {
        bar_line_text_result(st, &mut text, count_value, *max);
    }

    write!(text, " · @{}", view_state.agent).unwrap();
    spans.push(Span::raw(text));
}

fn card_line_flow_bar(receiver_state: &FlowBar) -> Option<(String, Style)> {
    let st = receiver_state.state.as_ref()?;

    if st.finished {
        return Some((
            format!("✓ {} finished", receiver_state.name),
            Style::default().fg(theme::pal().success()),
        ));
    }

    let step = receiver_state
        .nodes
        .get(st.node)
        .map(|count| count.title.clone())
        .unwrap_or_default();
    let mut text = format!(
        "{} {}/{} {step}",
        if st.wait.is_some() { "⏸" } else { "◆" },
        st.node + 1,
        receiver_state.nodes.len()
    );

    if !receiver_state.warnings.is_empty() {
        write!(text, " !{}", receiver_state.warnings.len()).unwrap();
    }

    // a switch at the next tool boundary
    if let Some(path) = &st.pending {
        write!(
            text,
            " → {}",
            receiver_state
                .lib
                .flow(&path.flow)
                .map_or(path.flow.clone(), |file| file.name.clone())
        )
        .unwrap();
    }

    let style = if st.wait.is_some() {
        theme::fg(theme::pal().attention())
    } else {
        theme::fg(theme::pal().progress())
    };

    Some((text, style))
}

fn bar_line_flow_bar(receiver_state: &FlowBar) -> (Vec<Span<'static>>, Style) {
    let Some(st) = &receiver_state.state else {
        return (
            vec![Span::raw(format!("◆ @{} · 0 flow: pick one", receiver_state.agent))],
            theme::muted(),
        );
    };

    let mut spans = Vec::new();
    let mut style = theme::accent();
    let total = receiver_state.nodes.len();
    let step = receiver_state
        .nodes
        .get(st.node)
        .map(|count| count.title.clone())
        .unwrap_or_default();

    if st.finished {
        spans.push(Span::raw(format!("◆ {} · finished", receiver_state.name)));
        style = Style::default().fg(theme::pal().success());
    } else {
        bar_line_text(st, &mut spans, total, &step, receiver_state);
    }

    finish_bar_line(st, spans, style, &step, receiver_state)
}
