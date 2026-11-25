use super::{
    App, Draft, Duration, Field, Frame, KeyCode, KeyEvent, Line, Modifier, Paragraph, Paster, PathBuf, Rect, Span,
    Style, UnicodeWidthStr, Where, claude, drafts, dropdown, theme, trash,
};

impl App {
    /// The task as it is now.
    pub(super) fn draft(&self) -> Draft {
        let place = self
            .cfg
            .repo
            .as_ref()
            .unwrap_or(&self.cfg.cwd)
            .file_name()
            .map(|count| count.to_string_lossy().into_owned())
            .unwrap_or_default();

        Draft {
            id: self.draft_selection.draft_id.clone(),
            saved: 0,
            place,
            title: self.compose.title.text(),
            prompt: self.compose.prompt.text(),
            notes: self.note_list.selected.clone(),
            picks: self.compose.picks.clone(),
            place_choice: self
                .repository
                .wheres
                .get(self.repository.where_i)
                .map(|width| format!("{width:?}"))
                .unwrap_or_default(),
            checkout: self.repository.checkout,
            stash: self.repository.stash,
            files: self
                .compose
                .files
                .iter()
                .filter(|(message, _)| self.compose.prompt.text().contains(message))
                .map(|(message_value, path)| (message_value.clone(), path.display().to_string()))
                .collect(),
            carry: self
                .compose
                .carry
                .iter()
                .map(|character| {
                    (
                        character.thread.clone(),
                        character.path.display().to_string(),
                        character.title.clone(),
                    )
                })
                .collect(),
            kept: self.draft_selection.draft_kept,
        }
    }

    /// After every key, paste, click: the task to its draft file, if it changed. 0 title and
    /// 0 prompt: 0 file.
    pub(super) fn backup(&mut self) {
        if self.cfg.copy_mode || self.outcome.is_some() {
            return;
        }

        let directory = self.draft();
        let content = directory.content();

        if content == self.draft_selection.draft_saved {
            return;
        }

        if directory.is_empty() {
            drafts::remove(&self.cfg.drafts, &self.draft_selection.draft_id);
        } else if let Err(error) = drafts::save(&self.cfg.drafts, &directory) {
            self.toast(format!("draft not saved: {error}"), true);
        }

        self.draft_selection.draft_saved = content;
    }

    /// The thread started: its draft goes.
    pub(crate) fn drop_draft(&self) {
        drafts::remove(&self.cfg.drafts, &self.draft_selection.draft_id);
    }

    /// ctrl+s: the task as a kept draft (0 cleanup by age), the form empty for the next one. The
    /// drafts row loads it later.
    pub(super) fn save_draft(&mut self) {
        let mut directory = self.draft();

        if directory.is_empty() {
            return self.toast("0 title, 0 prompt: nothing to save", true);
        }

        directory.kept = true;

        if let Err(error) = drafts::save(&self.cfg.drafts, &directory) {
            return self.toast(format!("draft not saved: {error}"), true);
        }

        let name = directory.name();

        // a new id first: clear_task drops the draft of the id on screen
        self.draft_selection.draft_id = drafts::new_id();
        self.clear_task();
        self.draft_selection.drafts = drafts::load(&self.cfg.drafts, &self.draft_selection.draft_id);
        self.toast(format!("draft saved: {name} · drafts row: load it"), false);
    }

    /// The thread started: the new task form empty again (defaults), with a draft id of its own.
    pub(crate) fn clear_task(&mut self) {
        self.drop_draft();
        self.compose.title.set_text("");
        self.compose.prompt.set_text("");
        self.note_list.selected.clear();
        self.compose.files.clear();
        self.compose.carry.clear();
        self.compose.target = None;
        self.repository.checkout = true;
        self.repository.stash = false;
        self.project_selection.proj_picks = None;
        self.project_selection.proj_menu = None;

        self.repository.where_i = if self.cfg.branch.is_some() {
            self.repository
                .wheres
                .iter()
                .position(|worktree| *worktree == Where::NewWorktree)
                .unwrap_or(0)
        } else {
            0
        };

        let mut picks: Vec<String> = self
            .cfg
            .picks
            .iter()
            .filter(|count| self.agent_list.agents.iter().any(|agent| &agent.name == *count))
            .cloned()
            .collect();

        if picks.is_empty()
            && self
                .agent_list
                .agents
                .iter()
                .any(|agent_value| agent_value.name == self.cfg.agent)
        {
            picks.push(self.cfg.agent.clone());
        }

        self.compose.picks = picks;
        let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());

        self.compose.paster = Paster::new(
            home.join(".cache/codehead/paste")
                .join(format!("new-{}", (crate::engine::now_ms() / 1000))),
        );
        self.draft_selection.draft_id = drafts::new_id();
        self.draft_selection.draft_kept = false;
        self.draft_selection.draft_saved = self.draft().content();
        self.compose.field = Field::Prompt;
    }

    /// "n" of the menu: the form empty, also in a draft. The task on screen stays a draft (its
    /// file: written by backup), listed in "view drafts (n)".
    pub(super) fn new_task(&mut self) {
        self.backup();

        // a new id first: clear_task drops the draft of the id on screen
        self.draft_selection.draft_id = drafts::new_id();
        self.clear_task();
        self.draft_selection.drafts = drafts::load(&self.cfg.drafts, &self.draft_selection.draft_id);
    }

    /// The button of the drafts row under the cursor: "view drafts" only with 1+ drafts.
    pub(super) fn draft_btn(&self) -> usize {
        if self.draft_selection.drafts.is_empty() {
            1
        } else {
            self.draft_selection.draft_btn.min(1)
        }
    }

    pub(super) fn open_drafts(&mut self) {
        self.draft_selection.drafts = drafts::load(&self.cfg.drafts, &self.draft_selection.draft_id);

        if self.draft_selection.drafts.is_empty() {
            self.compose.field = Field::Prompt;
            return;
        }

        // the latest (last) row is current
        self.draft_selection.drafts_menu = Some((self.draft_selection.drafts.len() - 1, 0));
    }

    pub(super) fn drafts_key(&mut self, key: KeyEvent) {
        let Some((cur, _)) = self.draft_selection.drafts_menu else {
            return;
        };

        let count = self.draft_selection.drafts.len();

        if count == 0 {
            self.draft_selection.drafts_menu = None;
            return;
        }

        let set = |app: &mut App, character: usize| {
            drafts_key_message(app, character);
        };

        let armed = self.draft_selection.draft_armed.take() == Some(cur);

        match key.code {
            KeyCode::Esc => self.draft_selection.drafts_menu = None,
            KeyCode::Up | KeyCode::Char('k') => set(self, (cur + count - 1) % count),
            KeyCode::Down | KeyCode::Char('j') => set(self, (cur + 1) % count),
            KeyCode::Enter => self.load_draft(cur),
            // d d: delete the draft (a 1st d asks)
            KeyCode::Char('d') | KeyCode::Delete if !armed => {
                self.draft_selection.draft_armed = Some(cur);
                self.toast("d again: delete the draft", false);
            }
            KeyCode::Char('d') | KeyCode::Delete => {
                drafts_key_directory(self, cur, count, set);
            }
            _ => {}
        }
    }

    /// Draft I into the task screen. The task on screen stays a draft of its own (its file is
    /// written already); the loaded draft's file is the task's file from now on.
    pub(super) fn load_draft(&mut self, i: usize) {
        self.draft_selection.drafts_menu = None;

        let Some(directory) = self.draft_selection.drafts.get(i).cloned() else {
            return;
        };

        self.draft_selection.draft_id.clone_from(&directory.id);
        self.draft_selection.draft_kept = directory.kept;
        self.compose.title.set_text(&directory.title);
        self.compose.prompt.set_text(&directory.prompt);
        self.note_list.selected = directory
            .notes
            .iter()
            .filter(|count| self.note_list.notes.files.contains(count))
            .cloned()
            .collect();
        let picks: Vec<String> = directory
            .picks
            .iter()
            .filter(|path_value| self.agent_list.agents.iter().any(|agent| &agent.name == *path_value))
            .cloned()
            .collect();

        if !picks.is_empty() {
            self.compose.picks = picks;
        }

        if let Some(width_value) = self
            .repository
            .wheres
            .iter()
            .position(|width| format!("{width:?}") == directory.place_choice)
        {
            self.repository.where_i = width_value;
        }

        self.repository.checkout = directory.checkout;
        self.repository.stash = directory.stash;
        self.compose.target = None;
        self.compose.files = directory
            .files
            .iter()
            .map(|(path_2, path_3)| (path_2.clone(), PathBuf::from(path_3)))
            .collect();

        // new pastes: numbers after the draft's markers
        for (message, _) in &self.compose.files {
            self.compose.paster.seen(message);
        }

        self.compose.carry = directory
            .carry
            .iter()
            .filter_map(|(thread, path, title)| match thread {
                Some(thread_value) => claude::of_thread(thread_value, title),
                None => claude::from_transcript(std::path::Path::new(path)),
            })
            .collect();
        self.compose.field = Field::Prompt;
        self.draft_selection.draft_saved = self.draft().content();
        self.draft_selection.drafts = drafts::load(&self.cfg.drafts, &self.draft_selection.draft_id);
        self.toast(format!("draft loaded: {}", directory.name()), false);
    }

    /// The open drafts dropdown, below its field.
    pub(super) fn draw_drafts(&mut self, frame: &mut Frame, bounds: Rect) {
        let Some((cur, off)) = self.draft_selection.drafts_menu else {
            return;
        };

        let count = self.draft_selection.drafts.len();

        // kept (ctrl+s): `★ saved` in the detail
        let rows: Vec<(String, String)> = self
            .draft_selection
            .drafts
            .iter()
            .map(|directory| {
                (
                    directory.name(),
                    format!(
                        "{}{} · {}",
                        if directory.kept { "★ saved · " } else { "" },
                        directory.place,
                        crate::domain::notes::ago(std::time::UNIX_EPOCH + Duration::from_secs(directory.saved))
                    ),
                )
            })
            .collect();

        let width = rows
            .iter()
            .map(|(agent, bytes)| agent.width().min(60) + bytes.width() + 5)
            .max()
            .unwrap_or(0)
            .clamp(44, 96) as u16;

        let Some(result) = dropdown::place(self.preview.regions.drafts, width, count, bounds, false) else {
            return;
        };

        let shown = (result.height as usize).saturating_sub(2);
        let off = dropdown::fit(off, cur, count, shown);

        self.draft_selection.drafts_menu = Some((cur, off));
        let inner = dropdown::frame(
            frame,
            result,
            "drafts",
            "enter: load · d d: delete · esc: back",
            dropdown::FrameState {
                up: off > 0,
                down: off + shown < count,
                active: true,
            },
        );

        draw_draft_rows(self, frame, cur, &rows, shown, off, inner);
    }
}

fn drafts_key_message(app: &mut App, character: usize) {
    if let Some(message) = &mut app.draft_selection.drafts_menu {
        message.0 = character;
    }
}

fn draw_draft_rows(
    app_state: &mut App,
    frame: &mut Frame<'_>,
    cur: usize,
    rows: &[(String, String)],
    shown: usize,
    off: usize,
    inner: Rect,
) {
    for (i, (name, detail)) in rows.iter().enumerate().skip(off).take(shown) {
        let row = Rect {
            y: inner.y + (i - off) as u16,
            height: 1,
            ..inner
        };
        let hl = i == cur;

        let st = if hl {
            theme::pill().add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::pal().text)
        };

        let dim = if hl { theme::pill() } else { theme::muted() };
        let room = (row.width as usize).saturating_sub(detail.width() + 4);

        let name: String = if name.width() > room {
            name.chars().take(room.saturating_sub(1)).chain(['…']).collect()
        } else {
            name.clone()
        };

        let pad = room.saturating_sub(name.width());
        let line = Line::from(vec![
            Span::styled(format!(" {name}"), st),
            Span::styled(" ".repeat(pad + 2), st),
            Span::styled(format!("{detail} "), dim),
        ]);

        frame.render_widget(Paragraph::new(line).style(st), row);
        app_state.preview.regions.draft_rows.push((row, i));
    }
}

fn drafts_key_directory(app_state: &mut App, cur: usize, count: usize, set: impl Fn(&mut App, usize)) {
    let directory = app_state.draft_selection.drafts.remove(cur.min(count - 1));

    match trash(&drafts::path(&app_state.cfg.drafts, &directory.id)) {
        Ok(how) => app_state.toast(format!("draft {how}: {}", directory.name()), false),
        Err(error) => app_state.toast(format!("draft not deleted: {error}"), true),
    }

    if app_state.draft_selection.drafts.is_empty() {
        app_state.draft_selection.drafts_menu = None;
        app_state.compose.field = Field::Prompt;
    } else {
        set(app_state, cur.min(app_state.draft_selection.drafts.len() - 1));
    }
}
