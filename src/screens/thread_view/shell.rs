use super::{
    KeyCode, KeyEvent, Msg, Pane, Path, PathBuf, SHELL_CTX_CHARS, TextArea, View, busiest_checkout, cd_target, chat,
    git_push, shell, store, theme, tilde,
};

impl View {
    /// The mode the keys are in, for the footer's badge: (label, risky). 0 mode: None.
    pub(super) fn mode(&self) -> Option<(&'static str, bool)> {
        let view_keys = !self.panes.prompt_focus;

        if self.agents.agents_focus.is_some() {
            Some(("agents", false))
        } else if self.find.open && self.find.focus {
            Some(("search", false))
        } else if self.panes.pane == Pane::Changes
            && view_keys
            && let Some(mode) = self.panes.changes.mode()
        {
            Some((mode, false))
        } else if self.panes.pane == Pane::Files && view_keys && self.panes.files.typing() {
            Some(("filter", false))
        } else if self.shell_mode() && (self.panes.pane == Pane::Chat || self.panes.prompt_focus) {
            Some(("shell", true))
        } else {
            None
        }
    }

    /// Shell mode: the prompt starts with `!`.
    pub(super) fn shell_mode(&self) -> bool {
        self.prompt.input.text().starts_with('!')
    }

    /// The thread's `!commands`, oldest first.
    pub(super) fn shell_cmds(&self) -> Vec<String> {
        self.thread.items.iter().filter_map(shell_command_of).collect()
    }

    /// Shell mode, not in the `!commands` (↑↓): the completions and the suggestion are on.
    pub(super) fn comp_on(&self) -> bool {
        self.shell_mode() && self.shell_state.shell_at.is_none() && !self.prompt.hist.is_open()
    }

    /// Run COMMAND (shell mode) in the worktree, off the UI thread: $SHELL -c, stdout and stderr
    /// together, at most `SHELL_TIMEOUT`; `Msg::Shell` when it ends.
    pub(super) fn run_shell(&mut self, command: String) {
        // `cd DIR` alone: shell mode's dir (every command runs in a new shell)
        if let Some(dir) = cd_target(&command) {
            self.set_shell_cwd(cd_dir(dir));
            return;
        }

        // a push of the checkout runs (the panel, an agent): 0 2nd push from shell mode (the
        // shim on its PATH holds the line too; this says so at once)
        if git_push(&command)
            && let Some(holder) = crate::infra::pushlock::git_dir(&self.shell_state.shell_cwd, &[])
                .and_then(|git_dir| crate::infra::pushlock::held_in(&git_dir))
        {
            self.toast(crate::infra::pushlock::busy_text(&holder));
            return;
        }

        self.shell_state.shell_running += 1;
        self.toast(format!("! {command}"));
        let (cwd, tx) = (self.shell_state.shell_cwd.clone(), self.tx.clone());

        std::thread::spawn(move || {
            let (output, code) = shell(&cwd, &command);
            let _ = tx.send(Msg::Shell(command, output, code, cwd));
        });
    }

    /// Shell mode's dir at the view's start: the owner's last pick, else (in the background) the
    /// thread's checkout with the most line changes.
    pub(super) fn load_shell_cwd(&mut self) {
        let saved = std::fs::read_to_string(store::thread_dir(&self.id).join("shell_cwd"))
            .ok()
            .map(|path| PathBuf::from(path.trim()))
            .filter(|path| path.is_dir());

        if let Some(path) = saved {
            self.shell_state.shell_cwd = path;
            return;
        }

        let Some(thread) = store::load(&self.id) else { return };
        let tx = self.tx.clone();

        let work = move || read_shell_checkout(&thread, &tx);

        if cfg!(test) {
            work();
        } else {
            std::thread::spawn(work);
        }
    }

    /// TYPED (`~/x`, `/x`, `x` from the dir now): shell mode's dir, kept for the thread. False: 0
    /// such dir.
    pub(super) fn set_shell_cwd(&mut self, typed: &str) -> bool {
        let wanted = typed.trim().trim_matches('"');

        let path = if wanted == "~" {
            crate::infra::config::home()
        } else if let Some(rest) = wanted.strip_prefix("~/") {
            crate::infra::config::home().join(rest)
        } else {
            self.shell_state.shell_cwd.join(wanted)
        };

        let Some(path) = std::fs::canonicalize(&path).ok().filter(|path| path.is_dir()) else {
            self.toast(format!("cd: 0 dir {wanted}"));
            return false;
        };

        let dir = store::thread_dir(&self.id);
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("shell_cwd"), path.display().to_string());

        self.toast(format!("shell mode: in {}", tilde(&path)));
        self.shell_state.shell_cwd = path;
        self.stream.dirty = true;

        true
    }

    /// ↓ at the bottom of shell mode: the dir input, the dir with a `/` (its entries show).
    pub(super) fn edit_cwd(&mut self) {
        let mut input = TextArea::new(true, "a dir");

        input.style = Some(theme::text());
        let dir = tilde(&self.shell_state.shell_cwd);

        let dir = if dir.ends_with('/') { dir } else { format!("{dir}/") };

        // a space in the path: quoted (the dropdown goes on inside the quote)
        input.set_text(&if dir.contains(char::is_whitespace) {
            format!("\"{dir}")
        } else {
            dir
        });
        self.shell_state.cwd_paths = crate::ui::paths::Paths::directory(&self.shell_state.shell_cwd);
        self.shell_state.cwd_paths.dirs_only = true;
        self.shell_state.cwd_edit = Some(input);
        self.stream.dirty = true;
    }

    /// KEY for the dir input (open). Enter: the dir (0 such dir: the input stays), esc / ↑:
    /// back to the prompt.
    pub(super) fn cwd_key(&mut self, key: KeyEvent) {
        let Some(mut input) = self.shell_state.cwd_edit.take() else {
            return;
        };

        if self.shell_state.cwd_paths.key(key, &mut input) {
            self.shell_state.cwd_edit = Some(input);
            return;
        }

        match key.code {
            KeyCode::Esc | KeyCode::Up => {
                self.shell_state.cwd_paths.reset_input();
                return;
            }
            KeyCode::Enter if accept_shell_cwd(self, &input.text()) => return,
            KeyCode::Enter => {}
            _ => {
                let _ = input.key(key);
            }
        }

        self.shell_state.cwd_edit = Some(input);
    }

    /// ↑↓ in shell mode: the thread's `!commands` (UP: older), like a shell. True: taken (↓ past
    /// the newest: the draft again; ↓ on the draft: false).
    pub(super) fn shell_history_step(&mut self, up: bool) -> bool {
        let mut cmds: Vec<String> = self.thread.items.iter().filter_map(shell_command_of).collect();

        // newest kept: a repeat counts once, at its last run
        let mut seen = std::collections::HashSet::new();

        cmds.reverse();
        cmds.retain(|command| seen.insert(command.clone()));
        cmds.reverse();

        let at = match (self.shell_state.shell_at, up) {
            (None, false) => return false,
            (None, true) if cmds.is_empty() => return true,
            (None, true) => {
                self.shell_state.shell_draft = self.prompt.input.text();
                cmds.len() - 1
            }
            (Some(i), true) => i.saturating_sub(1),
            (Some(i), false) if i + 1 < cmds.len() => i + 1,
            (Some(_), false) => {
                self.shell_state.shell_at = None;
                let draft = std::mem::take(&mut self.shell_state.shell_draft);

                self.prompt.input.load_text(&draft);
                return true;
            }
        };

        self.shell_state.shell_at = Some(at);
        self.prompt.input.load_text(&format!("! {}", cmds[at]));

        true
    }

    /// The shell COMMAND ended: into the thread's log (all see it), its output along with the
    /// owner's next message.
    pub(super) fn shell_done(&mut self, command: String, output: String, code: i32, dir: &Path) {
        self.shell_state.shell_running = self.shell_state.shell_running.saturating_sub(1);
        let cut: String = output.chars().take(SHELL_CTX_CHARS).collect();
        let more = if cut.len() < output.len() { "\n… (cut)" } else { "" };

        self.shell_state.shell_ctx.push(format!(
            "$ {command}  (exit {code}, in {})\n```\n{}{more}\n```",
            dir.display(),
            cut.trim_end()
        ));

        #[cfg(not(test))]
        if let Err(error) = store::append(
            &self.id,
            &store::Event::new(
                None,
                store::Kind::Shell {
                    command,
                    output,
                    code,
                    dir: Some(dir.display().to_string()),
                },
            ),
        ) {
            self.toast(format!("shell: {error}"));
        }

        #[cfg(test)]
        self.thread.items.push(chat::Item::Shell {
            command,
            output,
            code,
            ts: 0,
            dir: Some(dir.display().to_string()),
        });
        self.stream.dirty = true;
    }
}

fn shell_command_of(item: &chat::Item) -> Option<String> {
    if let chat::Item::Shell { command, .. } = item {
        Some(command.clone())
    } else {
        None
    }
}

fn cd_dir(target: &str) -> &str {
    if target.is_empty() { "~" } else { target }
}
fn read_shell_checkout(thread: &store::Thread, tx: &crate::infra::bus::Tx<Msg>) {
    if let Some(dir) = busiest_checkout(thread) {
        let _ = tx.send(Msg::ShellCwd(dir));
    }
}
fn accept_shell_cwd(view: &mut View, typed: &str) -> bool {
    if typed.trim().is_empty() || view.set_shell_cwd(typed) {
        view.shell_state.cwd_paths.reset_input();
        true
    } else {
        false
    }
}
