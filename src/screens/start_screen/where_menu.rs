use super::{
    App, Frame, KeyCode, KeyEvent, KeyModifiers, Rect, Target, UnicodeWidthStr, Where, WhereMenu, dropdown, fuzzy,
    tilde,
};

impl App {
    /// Text + detail of where choice W: a row of the where dropdown.
    pub(super) fn where_label(&self, width: &Where) -> (String, String) {
        match width {
            Where::Current => (
                "current branch".into(),
                if self.repository.here_branch.is_empty() {
                    "detached".into()
                } else {
                    self.repository.here_branch.clone()
                },
            ),
            Where::NewBranch => ("+ new branch".into(), "git switch -c · 0 worktrees".into()),
            Where::NewWorktree => ("+ new branch + worktree".into(), "git worktree add".into()),
            Where::Branch(bytes) => (
                bytes.clone(),
                self.repository
                    .checkouts
                    .get(bytes)
                    .map_or_else(|| "branch".into(), |path| format!("worktree {}", tilde(path))),
            ),
        }
    }

    /// Rows of the where dropdown, top to bottom (indexes of WHERES): the branches (the 1st choice,
    /// main or the newest, right above the rest), new branch + worktree, new branch, current
    /// branch at the bottom. With Q: the matches in that order by score, the best at the bottom.
    pub(super) fn where_rows(&self, query: &str) -> Vec<usize> {
        let mut value: Vec<usize> = (0..self.repository.wheres.len())
            .filter(|&i| matches!(self.repository.wheres[i], Where::Branch(_)))
            .rev()
            .collect();

        for worktree in [Where::NewWorktree, Where::NewBranch, Where::Current] {
            value.extend(self.repository.wheres.iter().position(|x| *x == worktree));
        }

        let query_value = query.trim().to_lowercase();

        if query_value.is_empty() {
            return value;
        }

        let mut message: Vec<(i64, usize, usize)> = value
            .into_iter()
            .enumerate()
            .filter_map(|(pos, i)| where_rows_text(self, &query_value, pos, i))
            .collect();

        message.sort_by_key(|&(text_2, pos, _)| (text_2, pos));

        message.into_iter().map(|(_, _, i)| i).collect()
    }

    /// Enter / space / click on the where row: the dropdown, its cursor on the current choice.
    pub(super) fn open_where(&mut self) {
        let rows = self.where_rows("");

        if rows.is_empty() {
            return;
        }

        let cursor = rows
            .iter()
            .position(|&i| i == self.repository.where_i)
            .unwrap_or(rows.len() - 1);

        self.repository.where_menu = Some(WhereMenu {
            cursor,
            offset: 0,
            query: String::new(),
        });
    }

    /// Choice I (of WHERES) from the dropdown. A target from the threads tab gives way to it.
    pub(super) fn pick_where(&mut self, i: usize) {
        self.repository.where_menu = None;
        self.repository.repo_pick = None;
        self.compose.target = None;
        self.repository.where_i = i;
    }

    /// Keys of the open where dropdown: ↑↓ move, type: filter, enter: pick, esc: close.
    pub(super) fn where_key(&mut self, key: KeyEvent) {
        let Some(query_value) = self.repository.where_menu.as_ref().map(|message| message.query.clone()) else {
            return;
        };

        let rows = self.where_rows(&query_value);
        let count = rows.len();
        let typed = !key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER);
        let mut query = None;

        let Some(message_value) = self.repository.where_menu.as_mut() else {
            return;
        };

        match key.code {
            KeyCode::Esc => self.repository.where_menu = None,
            KeyCode::Up if count > 0 => message_value.cursor = (message_value.cursor + count - 1) % count,
            KeyCode::Down if count > 0 => message_value.cursor = (message_value.cursor + 1) % count,
            KeyCode::PageUp => message_value.cursor = message_value.cursor.saturating_sub(dropdown::MAX_ROWS),
            KeyCode::PageDown if count > 0 => {
                message_value.cursor = (message_value.cursor + dropdown::MAX_ROWS).min(count - 1);
            }
            KeyCode::Enter => {
                let cursor = message_value.cursor;

                where_key_branch(self, &rows, cursor);
            }
            KeyCode::Backspace | KeyCode::Char('u' | 'w' | 'h')
                if { where_key_text(key, &query_value, &mut query) } => {}
            KeyCode::Char(character) if typed => query = Some(format!("{query_value}{character}")),
            _ => {}
        }

        // new filter: the cursor on the best match (bottom); 0 filter: on the current choice
        if let Some(query_2) = query {
            where_key_rows(self, query_2);
        }
    }

    /// Checkout box: (checked, enabled). Fixed on for the current branch and for branches that
    /// another checkout has (work goes there); off + disabled for new branches.
    pub(super) fn checkout_box(&self) -> (bool, bool) {
        if self.compose.target.is_some() {
            return (false, false);
        }

        match self.repository.wheres.get(self.repository.where_i) {
            Some(Where::Current) => (true, false),
            Some(Where::Branch(bytes)) if self.repository.checkouts.contains_key(bytes) => (true, false),
            Some(Where::Branch(_)) => (self.repository.checkout, true),
            _ => (false, false),
        }
    }

    /// Stash box: (checked, enabled). Only when the checkout box is on and a switch happens.
    pub(super) fn stash_box(&self) -> (bool, bool) {
        match self.checkout_box() {
            (true, true) => (self.repository.stash, true),
            _ => (false, false),
        }
    }

    /// Space / click on an opts box.
    pub(super) fn toggle_opt(&mut self, i: usize) {
        self.repository.opt_i = i;

        match (i, self.checkout_box(), self.stash_box()) {
            (0, (_, true), _) => self.repository.checkout = !self.repository.checkout,
            (1, _, (_, true)) => self.repository.stash = !self.repository.stash,
            (0, _, _) => self.toast("checkout: fixed for this choice", true),
            _ => self.toast("stash: only with a checkout", true),
        }
    }

    pub(super) fn where_text(&self) -> String {
        if let Some((url, path)) = self.clone_first() {
            let bytes_value = match self.target() {
                Ok(Target::Branch(bytes)) => bytes,
                _ => "…".into(),
            };

            return format!(
                "clones {url} into {}, then a new worktree · branch {bytes_value}",
                tilde(&path.display().to_string())
            );
        }

        if self.compose.target.is_none() && self.repository.wheres.get(self.repository.where_i) == Some(&Where::Current)
        {
            return format!("works in {} · 0 checkout", tilde(&self.repository.here_dir));
        }

        match self.target() {
            Ok(Target::Branch(bytes_2)) => format!("new worktree · branch {bytes_2}"),
            Ok(Target::Checkout {
                branch: bytes_3,
                switch,
                stash,
            }) => match self.repository.checkouts.get(&bytes_3) {
                Some(path_value) => format!("works in {} (checked out there)", tilde(path_value)),
                None if !switch => format!(
                    "works in {} on {} · 0 checkout",
                    self.repo_text(),
                    self.repository.repo_branch
                ),
                None if stash => format!(
                    "switches {} from {} to {bytes_3} · conflicting changes → git stash",
                    self.repo_text(),
                    self.repository.repo_branch
                ),
                None => format!(
                    "switches {} from {} to {bytes_3}",
                    self.repo_text(),
                    self.repository.repo_branch
                ),
            },
            Ok(Target::NewBranch(bytes_4)) => format!(
                "git switch -c {bytes_4} in {} (from {}) · 0 worktrees",
                self.repo_text(),
                self.repository.repo_branch
            ),
            Ok(Target::Worktree { branch, .. }) => format!("existing worktree · branch {branch}"),
            Ok(Target::InPlace) => format!("in place · {}", self.cfg.cwd.display()),
            Ok(Target::Dir(directory)) => format!("in place · {directory} (dir of the Claude Code session)"),
            Err(_) if self.repository.wheres.get(self.repository.where_i) == Some(&Where::NewBranch) => {
                format!("new branch {}/… (from the title)", self.cfg.prefix)
            }
            Err(_) => format!("new worktree · branch {}/… (from the title)", self.cfg.prefix),
        }
    }

    /// The open where dropdown, above the where chip (its rows grow upwards: current branch at the
    /// bottom, next to the chip). The current choice: `●`.
    pub(super) fn draw_where_menu(&mut self, frame: &mut Frame, bounds: Rect) {
        let Some((query, cur, off)) = self
            .repository
            .where_menu
            .as_ref()
            .map(|message| (message.query.clone(), message.cursor, message.offset))
        else {
            return;
        };

        let rows = self.where_rows(&query);
        let count = rows.len();

        let items = self.where_menu_items(&rows);

        let width = items
            .iter()
            .map(|result| result.text.width() + result.detail.width() + 8)
            .max()
            .unwrap_or(0)
            .clamp(40, 96) as u16;

        let Some(result_value) = dropdown::place(self.preview.regions.where_chip, width, count, bounds, true) else {
            return;
        };

        let cur = cur.min(count.saturating_sub(1));

        let label = if query.is_empty() {
            "where".to_string()
        } else {
            format!("where › {query}")
        };

        let note = if query.is_empty() {
            "type: filter".to_string()
        } else {
            format!("{count} of {}", self.repository.wheres.len())
        };

        let (off, drawn) = dropdown::list(
            frame,
            result_value,
            dropdown::List {
                label: &label,
                note: &note,
                rows: &items,
                cursor: cur,
                offset: off,
                empty: "0 matches",
            },
        );

        if let Some(message_value) = self.repository.where_menu.as_mut() {
            message_value.cursor = cur;
            message_value.offset = off;
        }

        self.preview.regions.where_rows = drawn.into_iter().map(|(row, j)| (row, rows[j])).collect();
    }

    fn where_menu_items(&self, rows: &[usize]) -> Vec<dropdown::Row> {
        let items: Vec<dropdown::Row> = rows
            .iter()
            .map(|&i| {
                let (text, detail) = self.where_label(&self.repository.wheres[i]);

                dropdown::Row {
                    text,
                    detail,
                    now: i == self.repository.where_i && self.compose.target.is_none(),
                    marks: Vec::new(),
                }
            })
            .collect();

        items
    }
}

fn where_key_text(key: KeyEvent, query_value: &str, query: &mut Option<String>) -> bool {
    let mut text = query_value.to_owned();

    crate::ui::textarea::delete_key(&mut text, &key) && {
        *query = Some(text);
        true
    }
}

fn where_key_rows(app_state: &mut App, query_2: String) {
    let rows = app_state.where_rows(&query_2);
    let cursor = if query_2.is_empty() {
        rows.iter().position(|&i| i == app_state.repository.where_i)
    } else {
        None
    }
    .unwrap_or(rows.len().saturating_sub(1));

    app_state.repository.where_menu = Some(WhereMenu {
        cursor,
        offset: 0,
        query: query_2,
    });
}

fn where_rows_text(app_state: &App, query_value: &str, pos: usize, i: usize) -> Option<(i64, usize, usize)> {
    let (label, detail) = app_state.where_label(&app_state.repository.wheres[i]);

    let text = if app_state.repository.wheres[i] == Where::Current {
        format!("{label} {detail}")
    } else {
        label
    };

    fuzzy(query_value, &text).map(|text_value| (text_value, pos, i))
}

fn where_key_branch(app_state: &mut App, rows: &[usize], cursor: usize) {
    if let Some(&i) = rows.get(cursor) {
        app_state.pick_where(i);
    }
}
