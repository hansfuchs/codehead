use super::{Instant, Mention, Paster, Path, PathBuf, TextArea, theme};

pub(super) struct PromptState {
    pub(super) input: TextArea,
    pub(super) mention: Mention,
    /// ctrl+r in shell mode: the history search.
    pub(super) hist: crate::features::prompt_input::history_search::HistSearch,
    pub(super) command: crate::features::prompt_input::command::Command,
    pub(super) paths: crate::ui::paths::Paths,
    /// Shell mode: the completions (tasks, commands, arguments), the history's suggestion.
    pub(super) comp: crate::features::prompt_input::shell_completion::ShellComp,
    /// `#` of the prompt: the shortcuts.
    pub(super) shortcuts: crate::ui::shortcuts::Pick,
    pub(super) paster: Paster,
    pub(super) attachments: Vec<(String, PathBuf)>,
    pub(super) history: Vec<String>,
    pub(super) hist_at: Option<usize>,
    pub(super) draft: String,
    /// The prompt draft as last written to disk (`prompt_path`): 0 write without a change.
    pub(super) prompt_saved: String,
    /// `@new` picked with "replace OLD": on send, OLD leaves and NEW gets the handoff.
    pub(super) replace: Option<(String, String)>,
    /// A replace waiting for OLD's turn to end: (new, old, message).
    pub(super) queued: Option<(String, String, String)>,
    /// Sent messages the log does not show yet: text, sent at, error.
    pub(super) pending: Vec<(String, Instant, Option<String>)>,
}

impl PromptState {
    pub(super) fn new(id: &str, cwd: &Path, roster: &[crate::domain::agents::Agent]) -> Self {
        let mut input = TextArea::new(false, "message the thread · @name for someone else");

        input.style = Some(theme::text());
        let mut command = crate::features::prompt_input::command::Command::new(cwd);

        command.can_branch = true;
        command.harnesses = roster.iter().map(|agent| agent.harness.clone()).collect();

        Self {
            input,
            mention: Mention::default(),
            hist: crate::features::prompt_input::history_search::HistSearch::default(),
            command,
            paths: crate::ui::paths::Paths::new(cwd),
            comp: crate::features::prompt_input::shell_completion::ShellComp::default(),
            shortcuts: crate::ui::shortcuts::Pick::default(),
            paster: Paster::new(crate::engine::data_dir().join("paste").join(id)),
            attachments: Vec::new(),
            history: Vec::new(),
            hist_at: None,
            draft: String::new(),
            prompt_saved: String::new(),
            replace: None,
            queued: None,
            pending: Vec::new(),
        }
    }
}
