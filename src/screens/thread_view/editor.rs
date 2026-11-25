use super::{Frame, KeyCode, KeyEvent, Path, PathBuf, Rect, View, store, tilde};

impl View {
    /// The repo roots the thread works in (its work dir 1st), with their labels
    /// (`repo ⎇ branch`); 0 repo: the work dir.
    pub(super) fn ide_dirs(&self) -> Vec<(PathBuf, String)> {
        let mut dirs: Vec<PathBuf> = Vec::new();
        let all = store::load(&self.id)
            .map(|thread| thread.all_checkouts())
            .unwrap_or_default();

        for path in all
            .iter()
            .map(|path| PathBuf::from(&path.dir))
            .filter(|directory| directory.is_dir())
            .chain([self.cwd.clone()])
        {
            ide_dirs_top(&mut dirs, &(path));
        }

        dirs.into_iter()
            .map(|directory| {
                let text = crate::screens::thread_view::workspot::spot(&directory, None);

                let label = match &text.branch {
                    Some(bytes) => format!("{} ⎇ {bytes}", text.repo),
                    None => text.repo,
                };

                (directory, label)
            })
            .collect()
    }

    /// ctrl+i / ⌘i: the IDE on the repo root; several repos: the picker above the repos line.
    pub(super) fn ide_key(&mut self) {
        let dirs = self.ide_dirs();

        match dirs.len() {
            0 => self.open_ide(&(self.cwd.clone())),
            1 => self.open_ide(&(dirs[0].0.clone())),
            _ => self.ide.ide_pick = Some((0, 0)),
        }
    }

    /// Keys of the IDE's repo picker: ↑↓ move, enter opens, esc closes.
    pub(super) fn ide_pick_key(&mut self, key: KeyEvent) {
        let Some((cur, off)) = self.ide.ide_pick else { return };
        let count = self.ide_dirs().len().max(1);

        match key.code {
            KeyCode::Esc => self.ide.ide_pick = None,
            KeyCode::Up | KeyCode::Char('k') => self.ide.ide_pick = Some(((cur + count - 1) % count, off)),
            KeyCode::Down | KeyCode::Char('j') => self.ide.ide_pick = Some(((cur + 1) % count, off)),
            KeyCode::Enter | KeyCode::Char(' ') => self.ide_pick_open(cur),
            _ => {}
        }
    }

    /// Repo I of the picker: the IDE on it, the picker closed.
    pub(super) fn ide_pick_open(&mut self, i: usize) {
        self.ide.ide_pick = None;

        if let Some((directory, _)) = self.ide_dirs().into_iter().nth(i) {
            self.open_ide(&(directory));
        }
    }

    /// The IDE on DIR (a window with DIR open: that one).
    pub(super) fn open_ide(&mut self, dir: &Path) {
        #[cfg(test)]
        {
            self.ide.ide_opened.push(dir.to_path_buf());
        }

        #[cfg(not(test))]
        self.open_editor_in(dir, |key| crate::infra::editor::args_dir(key, dir), None, tilde(dir));
    }

    /// The IDE's repo picker above the repos line (the row below the prompt), at its left.
    pub(super) fn draw_ide_pick(&mut self, frame: &mut Frame, bounds: Rect) {
        let Some((cur, off)) = self.ide.ide_pick else {
            self.ide.ide_rows.clear();
            return;
        };

        let dirs = self.ide_dirs();
        let rows: Vec<crate::ui::dropdown::Row> = dirs
            .iter()
            .map(|(directory, line)| crate::ui::dropdown::Row {
                text: line.clone(),
                detail: tilde(directory),
                now: *directory == crate::screens::thread_view::workspot::top(&self.cwd),
                marks: Vec::new(),
            })
            .collect();
        let width = rows
            .iter()
            .map(|result| result.text.chars().count() + result.detail.chars().count() + 8)
            .max()
            .unwrap_or(0)
            .clamp(30, 90) as u16;
        let anchor = Rect {
            x: self.layout.hint_rect.x + 2,
            width,
            ..self.layout.hint_rect
        };

        let Some(row) = crate::ui::dropdown::place(anchor, width, rows.len(), bounds, true) else {
            return;
        };

        let (off, drawn) = crate::ui::dropdown::list(
            frame,
            row,
            crate::ui::dropdown::List {
                label: "open in IDE",
                note: "enter: open · esc: close",
                rows: &rows,
                cursor: cur.min(rows.len().saturating_sub(1)),
                offset: off,
                empty: "0 repos",
            },
        );

        self.ide.ide_pick = Some((cur, off));
        self.ide.ide_rows = drawn;
    }

    /// Start the IDE with ARGS (by its kind); the toast names WHAT, else why not; once: the trust hint.
    /// TARGET (file, line; None: the worktree): a window that has the worktree open gets it.
    pub(super) fn open_editor(
        &mut self,
        args: impl Fn(crate::infra::editor::Kind) -> Vec<std::ffi::OsString>,
        target: Option<(&str, u32)>,
        what: String,
    ) {
        let wt = self.cwd.clone();

        self.open_editor_in(&wt, args, target, what);
    }

    /// [`Self::open_editor`] for the checkout WT.
    pub(super) fn open_editor_in(
        &mut self,
        wt: &Path,
        build_command: impl Fn(crate::infra::editor::Kind) -> Vec<std::ffi::OsString>,
        target: Option<(&str, u32)>,
        what: String,
    ) {
        // the setup tutorial: 0 IDE opens, the toast shows what would open
        if crate::infra::tutorial::active().is_some() {
            self.ide.tutor_ide = true;
            self.toast(format!("IDE: {what} · tutorial: your IDE opens here"));
            return;
        }

        let Some(error) = crate::infra::editor::find() else {
            self.toast("IDE: 0 found · setting editor (code, cursor, zed, idea …)");
            return;
        };

        // a terminal editor: full screen in this terminal (the view loop suspends codehead)
        if error.kind.in_terminal() {
            let mut argv: Vec<std::ffi::OsString> = vec![error.bin.clone().into()];

            argv.extend(build_command(error.kind));
            self.panes.outside = Some(argv);
            return;
        }

        let (argv, reused) = match crate::infra::editor::reuse_args(&error, wt, target) {
            Some(agent) => (agent, true),
            None => (build_command(error.kind), false),
        };

        let what = if reused { format!("{what} (open window)") } else { what };

        match crate::infra::editor::spawn(&error, &argv) {
            Ok(()) => {
                let hint = crate::infra::editor::trust_hint(wt);

                self.toast(hint.unwrap_or_else(|| format!("IDE: {what}")));
            }
            Err(err) => self.toast(format!("IDE: {err}")),
        }
    }
}

fn ide_dirs_top(dirs: &mut Vec<PathBuf>, path: &Path) {
    let top = crate::screens::thread_view::workspot::top(path);

    if !dirs.contains(&top) {
        dirs.push(top);
    }
}
