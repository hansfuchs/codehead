use super::{Focus, Mode, ThreadList};

impl ThreadList {
    /// Foot hints of the list.
    pub(crate) fn hints(&self) -> Vec<(&'static str, &'static str)> {
        hints_thread_list(self)
    }
}

fn hints_thread_list(state: &ThreadList) -> Vec<(&'static str, &'static str)> {
    if state.form.is_some() {
        return vec![
            ("↑↓ tab", "field"),
            ("←→", "repo"),
            ("space", "toggle"),
            ("ctrl+s", "save"),
            ("esc", "cancel"),
        ];
    }

    if state.job_state.pending.is_some() {
        return vec![("enter / y", "yes"), ("esc", "no")];
    }

    if state.job_state.actions.is_some() {
        return vec![
            ("↑↓", "move"),
            ("enter", "run"),
            ("letter", "its action"),
            ("esc", "close"),
        ];
    }

    match (state.mode, state.focus) {
        (_, Focus::Repos) => vec![("←→", "repo"), ("↓", "projects"), ("↑", "tabs")],
        (_, Focus::Projects) => vec![("←→", "project"), ("↑", "repos"), ("↓", "views")],
        (_, Focus::Views) => vec![
            ("←→", "view"),
            ("↑", "projects"),
            ("↓", if state.has_stats() { "status" } else { "list" }),
        ],
        (_, Focus::Status) => vec![("←→", "status"), ("↑", "views"), ("↓", "list")],
        (Mode::Switch, Focus::List) => vec![
            ("type", "filter"),
            ("←→", "repo"),
            ("↑↓", "move"),
            ("enter", "open"),
            ("ctrl+.", "actions"),
        ],
        (Mode::Manage, Focus::List) => vec![
            ("space", "select"),
            ("a", "archive"),
            ("u", "unarchive"),
            ("e", "edit"),
            ("d", "delete"),
            ("ctrl+.", "actions"),
        ],
    }
}
