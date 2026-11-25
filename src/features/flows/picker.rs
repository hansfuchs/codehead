use super::{FlowsTab, PickRow, flows};

impl FlowsTab {
    pub(super) fn pick_rows(&self, query: &str) -> Vec<PickRow> {
        pick_rows_flows_tab(self, query)
    }
}

fn step_score(query_lower: &str, step: &flows::Step) -> Option<(i64, String)> {
    if query_lower.is_empty() {
        return Some((0, step.name.clone()));
    }

    let score = if step.name == query_lower {
        exact_score()
    } else if step.name.starts_with(query_lower) {
        500
    } else if step.name.contains(query_lower) {
        300
    } else if step.description.to_lowercase().contains(query_lower) {
        100
    } else {
        return None;
    };

    Some((score, step.name.clone()))
}

fn exact_score() -> i64 {
    1000
}

fn push_query_rows(query: &str, rows: &mut Vec<PickRow>, hits: Vec<(i64, String)>) {
    rows.extend(hits.into_iter().map(|hit| PickRow::Step(hit.1)));

    if !query.is_empty() {
        rows.insert(0, PickRow::OneOff(query.to_string()));
        rows.insert(1, PickRow::NewStep(flows::slug(query)));
    }
}

fn pick_rows_flows_tab(state: &FlowsTab, query: &str) -> Vec<PickRow> {
    let trimmed = query.trim();
    let mut rows = Vec::new();
    let query_lower = trimmed.to_lowercase();

    let mut hits: Vec<(i64, String)> = state
        .lib
        .steps
        .iter()
        .filter_map(|step| step_score(&query_lower, step))
        .collect();

    hits.sort_by(|first, second| second.0.cmp(&first.0).then(first.1.cmp(&second.1)));
    let exact = hits.first().is_some_and(|hit| hit.0 == 1000);

    if exact {
        rows.extend(hits.into_iter().map(|hit| PickRow::Step(hit.1)));
        rows.push(PickRow::OneOff(trimmed.to_string()));
    } else {
        push_query_rows(trimmed, &mut rows, hits);
    }

    rows
}
