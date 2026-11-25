use super::{
    App, Frame, KeyCode, KeyEvent, KeyModifiers, Rect, Target, UnicodeWidthStr, Where, WhereMenu, dropdown, fuzzy,
    tilde,
};

impl App {
    /// Text + detail of where choice CHOICE: a row of the where dropdown.
    pub(super) fn where_label(&self, choice: &Where) -> (String, String) {
        match choice {
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
            Where::Branch(branch) => (
                branch.clone(),
                self.repository
                    .checkouts
                    .get(branch)
                    .map_or_else(|| "branch".into(), |path| format!("worktree {}", tilde(path))),
            ),
        }
    }

    /// Rows of the where dropdown, top to bottom (indexes of WHERES): the branches (the 1st choice,
    /// main or the newest, right above the rest), new branch + worktree, new branch, current
    /// branch at the bottom. With Q: the matches in that order by score, the best at the bottom.
    pub(super) fn where_rows(&self, query: &str) -> Vec<usize> {
        let mut rows: Vec<usize> = (0..self.repository.wheres.len())
            .filter(|&i| matches!(self.repository.wheres[i], Where::Branch(_)))
            .rev()
            .collect();

        for choice in [Where::NewWorktree, Where::NewBranch, Where::Current] {
            rows.extend(self.repository.wheres.iter().position(|x| *x == choice));
        }

        let needle = query.trim().to_lowercase();

        if needle.is_empty() {
            return rows;
        }

        let mut scored: Vec<(i64, usize, usize)> = rows
            .into_iter()
            .enumerate()
            .filter_map(|(pos, i)| where_row_score(self, &needle, pos, i))
            .collect();

        scored.sort_by_key(|&(score, pos, _)| (score, pos));

        scored.into_iter().map(|(_, _, i)| i).collect()
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
        let Some(current) = self.repository.where_menu.as_ref().map(|menu| menu.query.clone()) else {
            return;
        };

        let rows = self.where_rows(&current);
        let count = rows.len();
        let typed = !key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER);
        let mut query = None;

        let Some(menu) = self.repository.where_menu.as_mut() else {
            return;
        };

        match key.code {
            KeyCode::Esc => self.repository.where_menu = None,
            KeyCode::Up if count > 0 => menu.cursor = (menu.cursor + count - 1) % count,
            KeyCode::Down if count > 0 => menu.cursor = (menu.cursor + 1) % count,
            KeyCode::PageUp => menu.cursor = menu.cursor.saturating_sub(dropdown::MAX_ROWS),
            KeyCode::PageDown if count > 0 => {
                menu.cursor = (menu.cursor + dropdown::MAX_ROWS).min(count - 1);
            }
            KeyCode::Enter => {
                let cursor = menu.cursor;

                pick_where_row(self, &rows, cursor);
            }
            KeyCode::Backspace | KeyCode::Char('u' | 'w' | 'h') if { delete_from_query(key, &current, &mut query) } => {
            }
            KeyCode::Char(character) if typed => query = Some(format!("{current}{character}")),
            _ => {}
        }

        // new filter: the cursor on the best match (bottom); 0 filter: on the current choice
        if let Some(edited) = query {
            filter_where_menu(self, edited);
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
            Some(Where::Branch(branch)) if self.repository.checkouts.contains_key(branch) => (true, false),
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
            let branch = match self.target() {
                Ok(Target::Branch(branch)) => branch,
                _ => "…".into(),
            };

            return format!(
                "clones {url} into {}, then a new worktree · branch {branch}",
                tilde(&path.display().to_string())
            );
        }

        if self.compose.target.is_none() && self.repository.wheres.get(self.repository.where_i) == Some(&Where::Current)
        {
            return format!("works in {} · 0 checkout", tilde(&self.repository.here_dir));
        }

        match self.target() {
            Ok(Target::Branch(branch)) => format!("new worktree · branch {branch}"),
            Ok(Target::Checkout { branch, switch, stash }) => match self.repository.checkouts.get(&branch) {
                Some(path) => format!("works in {} (checked out there)", tilde(path)),
                None if !switch => format!(
                    "works in {} on {} · 0 checkout",
                    self.repo_text(),
                    self.repository.repo_branch
                ),
                None if stash => format!(
                    "switches {} from {} to {branch} · conflicting changes → git stash",
                    self.repo_text(),
                    self.repository.repo_branch
                ),
                None => format!(
                    "switches {} from {} to {branch}",
                    self.repo_text(),
                    self.repository.repo_branch
                ),
            },
            Ok(Target::NewBranch(branch)) => format!(
                "git switch -c {branch} in {} (from {}) · 0 worktrees",
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
            .map(|menu| (menu.query.clone(), menu.cursor, menu.offset))
        else {
            return;
        };

        let rows = self.where_rows(&query);
        let count = rows.len();

        let items = self.where_menu_items(&rows);

        let width = items
            .iter()
            .map(|row| row.text.width() + row.detail.width() + 8)
            .max()
            .unwrap_or(0)
            .clamp(40, 96) as u16;

        let Some(area) = dropdown::place(self.preview.regions.where_chip, width, count, bounds, true) else {
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
            area,
            dropdown::List {
                label: &label,
                note: &note,
                rows: &items,
                cursor: cur,
                offset: off,
                empty: "0 matches",
            },
        );

        if let Some(menu) = self.repository.where_menu.as_mut() {
            menu.cursor = cur;
            menu.offset = off;
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

fn delete_from_query(key: KeyEvent, current: &str, query: &mut Option<String>) -> bool {
    let mut text = current.to_owned();

    crate::ui::textarea::delete_key(&mut text, &key) && {
        *query = Some(text);
        true
    }
}

fn filter_where_menu(app: &mut App, query: String) {
    let rows = app.where_rows(&query);
    let cursor = if query.is_empty() {
        rows.iter().position(|&i| i == app.repository.where_i)
    } else {
        None
    }
    .unwrap_or(rows.len().saturating_sub(1));

    app.repository.where_menu = Some(WhereMenu {
        cursor,
        offset: 0,
        query,
    });
}

fn where_row_score(app: &App, needle: &str, pos: usize, i: usize) -> Option<(i64, usize, usize)> {
    let (label, detail) = app.where_label(&app.repository.wheres[i]);

    let text = if app.repository.wheres[i] == Where::Current {
        format!("{label} {detail}")
    } else {
        label
    };

    fuzzy(needle, &text).map(|score| (score, pos, i))
}

fn pick_where_row(app: &mut App, rows: &[usize], cursor: usize) {
    if let Some(&i) = rows.get(cursor) {
        app.pick_where(i);
    }
}
