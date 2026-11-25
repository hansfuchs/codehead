use super::{App, Frame, KeyCode, KeyEvent, KeyModifiers, Rect, UnicodeWidthStr, WhereMenu, dropdown, fuzzy};

impl App {
    /// The active projects of the new form's repo: read again when the projects dir changes
    /// (a brief made, saved, deleted: its mtime).
    pub(super) fn task_projects(&mut self) -> Vec<crate::domain::projects::Project> {
        let Some(repo) = self.cfg.repo.clone() else {
            return Vec::new();
        };

        let same_repo = self
            .project_selection
            .proj_cache
            .as_ref()
            .is_some_and(|character| character.0 == repo);

        let dir = match &self.project_selection.proj_cache {
            Some(character_value) if same_repo => character_value.1.clone(),
            _ => crate::domain::projects::dir(&repo),
        };

        let mtime = std::fs::metadata(&dir).and_then(|message| message.modified()).ok();

        if !same_repo
            || self
                .project_selection
                .proj_cache
                .as_ref()
                .is_some_and(|character_2| character_2.2 != mtime)
        {
            task_projects_all(self, repo, same_repo, dir, mtime);
        }

        self.project_selection
            .proj_cache
            .as_ref()
            .map(|character_3| character_3.3.clone())
            .unwrap_or_default()
    }

    /// Rows of the project dropdown: 0 = auto, I = the I-th project. With Q: the matches (title,
    /// slug, goal) by score, the best on top; auto only for 0 filter.
    pub(super) fn proj_rows(&mut self, query: &str) -> Vec<usize> {
        let all = self.task_projects();
        let query_value = query.trim().to_lowercase();

        if query_value.is_empty() {
            return (0..=all.len()).collect();
        }

        let mut message: Vec<(i64, usize)> = all
            .iter()
            .enumerate()
            .filter_map(|(i, path)| {
                fuzzy(&query_value, &format!("{} {} {}", path.title, path.slug, path.goal)).map(|text| (text, i + 1))
            })
            .collect();

        message.sort_unstable();

        message.into_iter().map(|(_, i)| i).collect()
    }

    /// Enter / space / click on the project row: the dropdown, its cursor on auto or the 1st pick.
    pub(super) fn open_projects(&mut self) {
        let all = self.task_projects();
        let cursor = self
            .project_selection
            .proj_picks
            .as_ref()
            .and_then(|value| value.first())
            .and_then(|text| all.iter().position(|path| path.slug == *text))
            .map_or(0, |i| i + 1);

        self.project_selection.proj_menu = Some(WhereMenu {
            cursor,
            offset: 0,
            query: String::new(),
        });
    }

    /// Row I of the project dropdown: 0 auto, else the I-th project on / off (0 on: auto again).
    /// CLOSE: the dropdown closes (enter, click); tab keeps it open, its filter cleared.
    pub(super) fn toggle_project(&mut self, i: usize, close: bool) {
        let all = self.task_projects();

        if i == 0 {
            self.project_selection.proj_picks = None;
        } else if let Some(path) = all.get(i - 1) {
            toggle_project_value(self, path);
        }

        self.project_selection.proj_menu = if close {
            None
        } else {
            Some(WhereMenu {
                cursor: i,
                offset: 0,
                query: String::new(),
            })
        };
    }

    /// Keys of the open project dropdown: ↑↓ move, type: filter, enter: pick + close, tab: pick,
    /// esc: close.
    pub(super) fn proj_key(&mut self, key: KeyEvent) {
        let Some(query_value) = self
            .project_selection
            .proj_menu
            .as_ref()
            .map(|message| message.query.clone())
        else {
            return;
        };

        let rows = self.proj_rows(&query_value);
        let count = rows.len();
        let typed = !key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER);
        let mut query = None;

        let Some(message_value) = self.project_selection.proj_menu.as_mut() else {
            return;
        };

        match key.code {
            KeyCode::Esc => self.project_selection.proj_menu = None,
            KeyCode::Up if count > 0 => message_value.cursor = (message_value.cursor + count - 1) % count,
            KeyCode::Down if count > 0 => message_value.cursor = (message_value.cursor + 1) % count,
            KeyCode::PageUp => message_value.cursor = message_value.cursor.saturating_sub(dropdown::MAX_ROWS),
            KeyCode::PageDown if count > 0 => {
                message_value.cursor = (message_value.cursor + dropdown::MAX_ROWS).min(count - 1);
            }
            KeyCode::Enter | KeyCode::Tab => {
                let cursor = message_value.cursor;

                select_project_row(self, &rows, cursor, key.code == KeyCode::Enter);
            }
            KeyCode::Backspace | KeyCode::Char('u' | 'w' | 'h') if { proj_key_text(key, &query_value, &mut query) } => {
            }
            KeyCode::Char(character) if typed => query = Some(format!("{query_value}{character}")),
            _ => {}
        }

        if let Some(query_2) = query {
            self.project_selection.proj_menu = Some(WhereMenu {
                cursor: 0,
                offset: 0,
                query: query_2,
            });
        }
    }

    /// The project chip's value: auto, or the picked titles.
    pub(super) fn proj_value(&mut self) -> String {
        let all = self.task_projects();

        match &self.project_selection.proj_picks {
            None => "auto".into(),
            Some(value) => value
                .iter()
                .map(|text| {
                    all.iter()
                        .find(|path| path.slug == *text)
                        .map_or(text.clone(), |path_value| path_value.title.clone())
                })
                .collect::<Vec<_>>()
                .join(", "),
        }
    }

    /// The open project dropdown below the chip (◼ picked), the brief of the current row in a 2nd
    /// box beside it.
    pub(super) fn draw_proj_menu(&mut self, frame: &mut Frame, bounds: Rect) {
        let Some((query, cur, off)) = self
            .project_selection
            .proj_menu
            .as_ref()
            .map(|message| (message.query.clone(), message.cursor, message.offset))
        else {
            return;
        };

        let all = self.task_projects();
        let rows = self.proj_rows(&query);
        let count = rows.len();
        let picks = self.project_selection.proj_picks.clone();

        let items = project_dropdown_items(&all, &rows, picks.as_deref());

        let width = items
            .iter()
            .map(|result| result.text.width() + result.detail.width() + 8)
            .max()
            .unwrap_or(0)
            .clamp(40, 72) as u16;

        let Some(result_value) = dropdown::place(self.preview.regions.proj_chip, width, count, bounds, false) else {
            return;
        };

        let cur = cur.min(count.saturating_sub(1));

        let (label, note) = project_filter_labels(&query, &all, count);

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

        if let Some(message_value) = self.project_selection.proj_menu.as_mut() {
            message_value.cursor = cur;
            message_value.offset = off;
        }

        // the 2nd box: the brief of the hovered row, else of the current one
        let hover = self.preview.hover.and_then(|height| {
            drawn
                .iter()
                .find(|(result_2, _)| result_2.contains(height))
                .map(|&(_, j)| j)
        });

        let j = hover.unwrap_or(cur);

        self.preview.regions.proj_rows = drawn.iter().map(|&(row, j)| (row, rows[j])).collect();

        if let Some(path_value) = rows.get(j).filter(|&&i| i > 0).map(|&i| &all[i - 1]) {
            draw_proj_menu_text(frame, bounds, result_value, &drawn, j, path_value);
        }
    }
}

fn proj_key_text(key: KeyEvent, query_value: &str, query: &mut Option<String>) -> bool {
    let mut text = query_value.to_owned();

    crate::ui::textarea::delete_key(&mut text, &key) && {
        *query = Some(text);
        true
    }
}

fn draw_proj_menu_text(
    frame: &mut Frame<'_>,
    bounds: Rect,
    result_value: Rect,
    drawn: &[(Rect, usize)],
    j: usize,
    path_value: &crate::domain::projects::Project,
) {
    let text = crate::domain::projects::body_of(&std::fs::read_to_string(&path_value.path).unwrap_or_default());
    let text = text.trim_end();

    let text = if text.is_empty() {
        "0 brief yet: projects tab (F3)"
    } else {
        text
    };

    let (bw, br) = crate::ui::shortcuts::box_size(text, bounds);
    let row_y = drawn
        .iter()
        .find(|(_, i)| *i == j)
        .map_or(result_value.y + 1, |(rr, _)| rr.y);

    if let Some(bytes) = dropdown::place_beside(result_value, row_y, bw, br, bounds, result_value) {
        crate::ui::shortcuts::text_box(frame, bytes, &path_value.title, text);
    }
}

fn project_filter_labels(query: &str, all: &[crate::domain::projects::Project], count: usize) -> (String, String) {
    let label = if query.is_empty() {
        "project".to_string()
    } else {
        format!("project › {query}")
    };

    let note = if query.is_empty() {
        "type: filter · tab: add".to_string()
    } else {
        format!("{count} of {}", all.len() + 1)
    };

    (label, note)
}

fn project_dropdown_items(
    all: &[crate::domain::projects::Project],
    rows: &[usize],
    picks: Option<&[String]>,
) -> Vec<dropdown::Row> {
    let items: Vec<dropdown::Row> = rows
        .iter()
        .map(|&i| {
            if i == 0 {
                dropdown::Row {
                    text: format!("{} auto", if picks.is_none() { "◼" } else { "◻" }),
                    detail: "agents assign projects".into(),
                    ..Default::default()
                }
            } else {
                let path = &all[i - 1];
                let on = picks.as_ref().is_some_and(|value| value.contains(&path.slug));

                dropdown::Row {
                    text: format!("{} {}", if on { "◼" } else { "◻" }, path.title),
                    detail: path.slug.clone(),
                    ..Default::default()
                }
            }
        })
        .collect();

    items
}

fn task_projects_all(
    app_state: &mut App,
    repo: std::path::PathBuf,
    same_repo: bool,
    dir: std::path::PathBuf,
    mtime: Option<std::time::SystemTime>,
) {
    let all: Vec<crate::domain::projects::Project> = crate::domain::projects::list_in(&dir)
        .into_iter()
        .filter(|path| !path.done)
        .collect();

    // another repo: its own projects, auto again
    if !same_repo && app_state.project_selection.proj_cache.is_some() {
        app_state.project_selection.proj_picks = None;
        app_state.project_selection.proj_menu = None;
    }

    app_state.project_selection.proj_cache = Some((repo, dir, mtime, all));
}

fn toggle_project_value(app_state: &mut App, path: &crate::domain::projects::Project) {
    let mut value = app_state.project_selection.proj_picks.clone().unwrap_or_default();

    match value.iter().position(|x| *x == path.slug) {
        Some(j) => {
            value.remove(j);
        }
        None => value.push(path.slug.clone()),
    }

    app_state.project_selection.proj_picks = (!value.is_empty()).then_some(value);
}

fn select_project_row(app_state: &mut App, rows: &[usize], cursor: usize, close: bool) {
    if let Some(&index) = rows.get(cursor) {
        app_state.toggle_project(index, close);
    }
}
