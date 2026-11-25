use super::{Path, PathBuf, TextArea};

pub(super) struct ShellState {
    /// Shell mode: the owner's commands since the last message (the next one takes them to the
    /// agents); the commands running now.
    pub(super) shell_ctx: Vec<String>,
    pub(super) shell_running: usize,
    /// Shell mode's dir: the owner's pick (`<thread dir>/shell_cwd`), else the thread's checkout
    /// with the most line changes.
    pub(super) shell_cwd: PathBuf,
    /// ↓ at the bottom of shell mode: the dir input (its own path dropdown, dirs only).
    pub(super) cwd_edit: Option<TextArea>,
    pub(super) cwd_paths: crate::ui::paths::Paths,
    /// ↑↓ in shell mode: the thread's `!commands`, this one in view (None: the draft).
    pub(super) shell_at: Option<usize>,
    pub(super) shell_draft: String,
}

impl ShellState {
    pub(super) fn new(cwd: &Path) -> Self {
        Self {
            shell_ctx: Vec::new(),
            shell_running: 0,
            shell_cwd: cwd.to_path_buf(),
            cwd_edit: None,
            cwd_paths: crate::ui::paths::Paths::default(),
            shell_at: None,
            shell_draft: String::new(),
        }
    }
}
