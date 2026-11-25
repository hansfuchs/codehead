//! The projects tab of the start screen: the projects of a repo (their briefs, `projects.rs`).
//! A repo row (←→ on it), the list, the brief of the project under the cursor (or its form).
//! Keys on the list: n new · enter / e edit · x done / active · d d delete · o open in the IDE.
//! The form (n, enter): a title field, a brief textarea, merged into the brief's Markdown file
//! (title in the front matter); tab switch field, ctrl+s save, esc close (edits: a 2nd esc drops them).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use crate::domain::projects::{self, Project};
use crate::ui::tab_group::TabGroup;
use crate::ui::textarea::TextArea;
use crate::ui::theme;

/// What the host does after a key / click.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Out {
    None,
    Toast(String),
    /// ↑ above the repo row: the tab bar takes the keys.
    Up,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Focus {
    Repos,
    List,
}

pub(crate) struct ProjectsTab {
    /// Repos to show (the start screen's first).
    pub repos: Vec<PathBuf>,
    repo_i: usize,
    tabs: TabGroup,
    focus: Focus,
    pub items: Vec<Project>,
    /// Active threads: (repo, project slugs) each (the host's cards).
    pub threads: Vec<(PathBuf, Vec<String>)>,
    cursor: usize,
    /// The form: a new project (n) or a brief to edit (enter).
    form: Option<Form>,
    /// The last esc in the form with edits: a 2nd within 2 s drops them.
    esc_at: Option<Instant>,
    /// d once: a 2nd d deletes.
    del: bool,
    list: Rect,
}

/// The title field and the brief textarea of a project.
struct Form {
    title: TextArea,
    body: TextArea,
    on_title: bool,
    /// The project edited; None: a new project.
    slug: Option<String>,
}

impl Form {
    fn new(slug: Option<String>, title: &str, body: &str) -> Form {
        let mut title_input = TextArea::new(true, "project title, e.g. Website refinement");

        title_input.style = Some(theme::text());
        title_input.set_text(title);
        let mut brief_input = TextArea::new(false, "the brief: goal, decisions, open points");

        brief_input.style = Some(theme::text());
        brief_input.load_text(body);

        // the cursor: line 1's end (a new brief: after `Goal: `)
        brief_input.move_start();
        brief_input.key(KeyEvent::from(KeyCode::End));

        Form {
            title: title_input,
            body: brief_input,
            on_title: slug.is_none(),
            slug,
        }
    }
}

impl Default for ProjectsTab {
    fn default() -> Self {
        ProjectsTab {
            repos: Vec::new(),
            repo_i: 0,
            tabs: TabGroup::default(),
            focus: Focus::List,
            items: Vec::new(),
            threads: Vec::new(),
            cursor: 0,
            form: None,
            esc_at: None,
            del: false,
            list: Rect::default(),
        }
    }
}

impl ProjectsTab {
    /// The tab opens: REPOS (first: the start screen's repo), read the briefs.
    pub(crate) fn enter(&mut self, repos: Vec<PathBuf>) {
        let cur = self.repo().cloned();

        self.repos = repos;
        self.repo_i = cur
            .and_then(|character| self.repos.iter().position(|result| *result == character))
            .unwrap_or(0);
        self.reload();
    }

    pub(crate) fn repo(&self) -> Option<&PathBuf> {
        self.repos.get(self.repo_i)
    }

    pub(crate) fn reload(&mut self) {
        self.items = self.repo().map(|result| projects::list(result)).unwrap_or_default();
        self.cursor = self.cursor.min(self.items.len().saturating_sub(1));
    }

    /// An input takes the keys (esc stays here).
    pub(crate) fn typing(&self) -> bool {
        self.form.is_some() || self.del
    }

    fn count(&self, slug: &str) -> usize {
        let repo = self.repo();

        self.threads
            .iter()
            .filter(|(result, ps)| Some(result) == repo && ps.iter().any(|path| path == slug))
            .count()
    }

    fn open_editor(&mut self) {
        let Some(path_value) = self.items.get(self.cursor) else {
            return;
        };

        let text = std::fs::read_to_string(&path_value.path).unwrap_or_default();

        self.form = Some(Form::new(
            Some(path_value.slug.clone()),
            &path_value.title,
            &projects::body_of(&text),
        ));
        self.esc_at = None;
    }

    /// ctrl+s: title + brief → the Markdown file (a new project: made first).
    fn save(&mut self) -> Out {
        save_projects_tab(self)
    }

    fn delete(&mut self) -> Out {
        delete_projects_tab(self)
    }

    pub(crate) fn paste(&mut self, text: &str) {
        if let Some(fm) = self.form.as_mut() {
            paste_if_fm_on_title(text, fm);
        }
    }

    pub(crate) fn help() -> &'static [(&'static str, &'static str)] {
        &[
            ("←→", "repo"),
            ("↑↓", "project"),
            ("n", "new project (its brief: <home repo>/<repo>/projects/<slug>.md)"),
            (
                "n / enter / e",
                "form: title, brief · tab field · ctrl+s save · esc close",
            ),
            ("x", "done / active (done: out of the pickers)"),
            ("d d", "delete the brief"),
            ("o", "open the brief in the IDE"),
        ]
    }

    pub(crate) fn hints(&self) -> &'static [(&'static str, &'static str)] {
        hints_projects_tab(self)
    }
}

fn paste_if_fm_on_title(text: &str, fm: &mut Form) {
    if fm.on_title {
        fm.title.insert(&text.replace('\n', " "));
    } else {
        fm.body.paste(text);
    }
}

#[cfg(test)]
#[path = "projects_tests.rs"]
mod tests;

#[path = "projects_draw.rs"]
mod draw;
#[path = "projects_key.rs"]
mod key;
#[path = "projects_mouse.rs"]
mod mouse;

impl Form {
    fn edited(&self) -> bool {
        self.title.edited() || self.body.edited()
    }
}

fn step_repo(directory: isize, model_state: &mut ProjectsTab) {
    if model_state.repos.is_empty() {
        return;
    }

    model_state.repo_i =
        (model_state.repo_i as isize + directory).rem_euclid(model_state.repos.len() as isize) as usize;
    model_state.cursor = 0;
    model_state.reload();
}

fn open_ide(model_state: &mut ProjectsTab) -> Out {
    let Some(path_value) = model_state.items.get(model_state.cursor) else {
        return Out::None;
    };

    let Some(error) = crate::infra::editor::find() else {
        return Out::Toast("0 editor found (setting `editor`): enter edits here".into());
    };

    if error.kind.in_terminal() {
        return Out::Toast("terminal editor: enter edits here".into());
    }

    let dir = path_value.path.parent().map(PathBuf::from).unwrap_or_default();
    let file = path_value
        .path
        .file_name()
        .map(|count| count.to_string_lossy().into_owned())
        .unwrap_or_default();

    match crate::infra::editor::spawn(&error, &crate::infra::editor::args_at(error.kind, &dir, &file, 1)) {
        Ok(()) => Out::Toast(format!("{} open in the editor · r: read again", path_value.slug)),
        Err(err) => Out::Toast(err),
    }
}

fn save_projects_tab(receiver_state: &mut ProjectsTab) -> Out {
    let Some(fm) = receiver_state.form.as_ref() else {
        return Out::None;
    };

    let title = fm.title.text().trim().to_string();

    if title.is_empty() {
        return Out::Toast("title: 1 letter or digit at least".into());
    }

    let body = fm.body.text();

    let (path, made) = if let Some(slug) = &fm.slug {
        match receiver_state.items.iter().find(|x| &x.slug == slug) {
            Some(path_value) => (path_value.path.clone(), false),
            None => return Out::Toast(format!("{slug}: gone")),
        }
    } else {
        let Some(repo) = receiver_state.repo().cloned() else {
            return Out::None;
        };

        match projects::create_in(&projects::dir(&repo), &title) {
            Ok(path_2) => (path_2.path, true),
            Err(error) => return Out::Toast(error),
        }
    };

    let old = std::fs::read_to_string(&path).unwrap_or_default();

    if let Err(error_value) = crate::engine::write_atomic(&path, projects::merge(&old, &title, &body).as_bytes()) {
        return Out::Toast(format!("{}: {error_value}", path.display()));
    }

    let slug = path
        .file_stem()
        .map(|text_value| text_value.to_string_lossy().into_owned())
        .unwrap_or_default();

    receiver_state.form = None;
    receiver_state.esc_at = None;
    receiver_state.reload();
    receiver_state.cursor = receiver_state.items.iter().position(|x| x.slug == slug).unwrap_or(0);
    receiver_state.focus = Focus::List;

    Out::Toast(if made {
        format!("project {slug} made")
    } else {
        format!("{slug}: saved")
    })
}

fn delete_projects_tab(receiver_state: &mut ProjectsTab) -> Out {
    let Some(path_value) = receiver_state.items.get(receiver_state.cursor).cloned() else {
        return Out::None;
    };

    if let Err(error) = std::fs::remove_file(&path_value.path) {
        return Out::Toast(format!("{}: {error}", path_value.path.display()));
    }

    receiver_state.reload();
    let count_value = receiver_state.count(&path_value.slug);

    Out::Toast(if count_value > 0 {
        format!(
            "{} deleted · {count_value} thread(s) keep its name until edited",
            path_value.slug
        )
    } else {
        format!("{} deleted", path_value.slug)
    })
}

fn hints_projects_tab(receiver_state: &ProjectsTab) -> &'static [(&'static str, &'static str)] {
    if let Some(fm) = &receiver_state.form {
        if fm.slug.is_none() {
            &[("tab", "field"), ("ctrl+s", "make"), ("esc", "cancel")]
        } else {
            &[("tab", "field"), ("ctrl+s", "save"), ("esc", "close")]
        }
    } else if receiver_state.del {
        &[("d", "delete"), ("any key", "keep")]
    } else {
        &[
            ("←→", "repo"),
            ("n", "new"),
            ("enter", "edit"),
            ("x", "done"),
            ("d d", "delete"),
            ("o", "IDE"),
        ]
    }
}
