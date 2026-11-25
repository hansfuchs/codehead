use super::{FlowsTab, PickRow, flows};

impl FlowsTab {
    pub(super) fn pick_rows(&self, query: &str) -> Vec<PickRow> {
        pick_rows_flows_tab(self, query)
    }
}

fn pick_rows_score(ql: &str, text: &flows::Step) -> Option<(i64, String)> {
    if ql.is_empty() {
        return Some((0, text.name.clone()));
    }

    let score = if text.name == ql {
        pick_rows_body_state()
    } else if text.name.starts_with(ql) {
        500
    } else if text.name.contains(ql) {
        300
    } else if text.description.to_lowercase().contains(ql) {
        100
    } else {
        return None;
    };

    Some((score, text.name.clone()))
}

fn pick_rows_body_state() -> i64 {
    1000
}

fn pick_rows_body(query_value: &str, value: &mut Vec<PickRow>, hits: Vec<(i64, String)>) {
    value.extend(hits.into_iter().map(|height_2| PickRow::Step(height_2.1)));

    if !query_value.is_empty() {
        value.insert(0, PickRow::OneOff(query_value.to_string()));
        value.insert(1, PickRow::NewStep(flows::slug(query_value)));
    }
}

fn pick_rows_flows_tab(state: &FlowsTab, query: &str) -> Vec<PickRow> {
    let query_value = query.trim();
    let mut value = Vec::new();
    let ql = query_value.to_lowercase();

    let mut hits: Vec<(i64, String)> = state
        .lib
        .steps
        .iter()
        .filter_map(|text| pick_rows_score(&ql, text))
        .collect();

    hits.sort_by(|agent, bytes| bytes.0.cmp(&agent.0).then(agent.1.cmp(&bytes.1)));
    let exact = hits.first().is_some_and(|height| height.0 == 1000);

    if exact {
        value.extend(hits.into_iter().map(|height_value| PickRow::Step(height_value.1)));
        value.push(PickRow::OneOff(query_value.to_string()));
    } else {
        pick_rows_body(query_value, &mut value, hits);
    }

    value
}
