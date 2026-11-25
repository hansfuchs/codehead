use super::{FlowsTab, Focus, Sub};

impl FlowsTab {
    pub(crate) fn help(&self) -> &'static [(&'static str, &'static str)] {
        help_flows_tab(self)
    }
}

fn help_flows_tab(state: &FlowsTab) -> &'static [(&'static str, &'static str)] {
    if state.ask.is_some() {
        return &[("enter", "ok"), ("esc", "cancel")];
    }

    if state.picker.is_some() {
        return &[
            ("type", "find a step / write a one-off"),
            ("↑↓", "move"),
            ("enter", "add below"),
            ("esc", "close"),
        ];
    }

    if state.target.is_some() {
        return &[("↑↓", "pick the target step"), ("enter", "ok"), ("esc", "cancel")];
    }

    if state.filtering {
        return &[("type", "filter"), ("enter", "done"), ("esc", "clear")];
    }

    match state.focus {
        Focus::List if state.armed => &[("d", "again: delete"), ("any key", "keep")],
        Focus::List if state.sub == Sub::Flows => FLOW_LIST_HELP,
        Focus::List => STEP_LIST_HELP,
        Focus::Rail if state.armed => &[("d", "again: remove the step"), ("any key", "keep")],
        Focus::Rail => RAIL_HELP,
        Focus::Editor => EDITOR_HELP,
    }
}

const FLOW_LIST_HELP: &[(&str, &str)] = &[
    ("↑↓", "move"),
    ("enter", "rail"),
    ("←→", "steps"),
    ("n", "new"),
    ("D", "duplicate"),
    ("R", "rename"),
    ("d d", "delete"),
    ("r", "read again"),
    ("ctrl+d", "reset"),
    ("m", "user ↔ repo"),
    ("ctrl+f", "filter"),
];
const STEP_LIST_HELP: &[(&str, &str)] = &[
    ("↑↓", "move"),
    ("enter", "edit"),
    ("←→", "flows"),
    ("n", "new"),
    ("D", "duplicate"),
    ("R", "rename"),
    ("d d", "delete"),
    ("r", "read again"),
    ("ctrl+d", "reset"),
    ("ctrl+f", "filter"),
];
const RAIL_HELP: &[(&str, &str)] = &[
    ("a", "add"),
    ("enter", "edit"),
    ("J K", "move"),
    ("d d", "remove"),
    ("g", "gate"),
    ("o / O", "outcome / clear"),
    ("L", "loop"),
    ("+ -", "max"),
    ("s", "save as step"),
    ("esc", "list"),
];
const EDITOR_HELP: &[(&str, &str)] = &[
    ("tab", "next field"),
    ("ctrl+g", "gate"),
    ("ctrl+f", "fork shared → one-off"),
    ("#", "shortcuts"),
    ("ctrl+s", "save"),
    ("esc", "close"),
];
