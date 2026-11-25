use super::{Pane, Path, PathBuf};

pub(super) struct PaneState {
    pub(super) pane: Pane,
    pub(super) changes: crate::features::changes::View,
    pub(super) files: crate::features::files::View,
    /// yazi on this path: the view loop runs it full screen (codehead suspends).
    pub(super) yazi: Option<PathBuf>,
    /// A program to run full screen (a terminal editor): the view loop runs it.
    pub(super) outside: Option<Vec<std::ffi::OsString>>,
    pub(super) prompt_focus: bool,
}

impl PaneState {
    pub(super) fn new(id: &str, cwd: &Path) -> Self {
        Self {
            pane: Pane::Chat,
            changes: crate::features::changes::View::new(id, cwd),
            files: crate::features::files::View::new(id, cwd),
            yazi: None,
            outside: None,
            prompt_focus: false,
        }
    }
}
