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
        let previous = self.repo().cloned();

        self.repos = repos;
        self.repo_i = previous
            .and_then(|previous| self.repos.iter().position(|repo| *repo == previous))
            .unwrap_or(0);
        self.reload();
    }

    pub(crate) fn repo(&self) -> Option<&PathBuf> {
        self.repos.get(self.repo_i)
    }

    pub(crate) fn reload(&mut self) {
        self.items = self.repo().map(|repo| projects::list(repo)).unwrap_or_default();
        self.cursor = self.cursor.min(self.items.len().saturating_sub(1));
    }

    /// An input takes the keys (esc stays here).
    pub(crate) fn typing(&self) -> bool {
        self.form.is_some() || self.del
    }

    fn thread_count(&self, slug: &str) -> usize {
        let repo = self.repo();

        self.threads
            .iter()
            .filter(|(thread_repo, slugs)| {
                Some(thread_repo) == repo && slugs.iter().any(|thread_slug| thread_slug == slug)
            })
            .count()
    }

    fn open_editor(&mut self) {
        let Some(project) = self.items.get(self.cursor) else {
            return;
        };

        let text = std::fs::read_to_string(&project.path).unwrap_or_default();

        self.form = Some(Form::new(
            Some(project.slug.clone()),
            &project.title,
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
        if let Some(form) = self.form.as_mut() {
            paste_into_form(text, form);
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

fn paste_into_form(text: &str, form: &mut Form) {
    if form.on_title {
        form.title.insert(&text.replace('\n', " "));
    } else {
        form.body.paste(text);
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

fn step_repo(delta: isize, tab: &mut ProjectsTab) {
    if tab.repos.is_empty() {
        return;
    }

    tab.repo_i = (tab.repo_i as isize + delta).rem_euclid(tab.repos.len() as isize) as usize;
    tab.cursor = 0;
    tab.reload();
}

fn open_ide(tab: &mut ProjectsTab) -> Out {
    let Some(project) = tab.items.get(tab.cursor) else {
        return Out::None;
    };

    let Some(editor) = crate::infra::editor::find() else {
        return Out::Toast("0 editor found (setting `editor`): enter edits here".into());
    };

    if editor.kind.in_terminal() {
        return Out::Toast("terminal editor: enter edits here".into());
    }

    let dir = project.path.parent().map(PathBuf::from).unwrap_or_default();
    let file = project
        .path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();

    match crate::infra::editor::spawn(&editor, &crate::infra::editor::args_at(editor.kind, &dir, &file, 1)) {
        Ok(()) => Out::Toast(format!("{} open in the editor · r: read again", project.slug)),
        Err(err) => Out::Toast(err),
    }
}

fn save_projects_tab(tab: &mut ProjectsTab) -> Out {
    let Some(form) = tab.form.as_ref() else {
        return Out::None;
    };

    let title = form.title.text().trim().to_string();

    if title.is_empty() {
        return Out::Toast("title: 1 letter or digit at least".into());
    }

    let body = form.body.text();

    let (path, made) = if let Some(slug) = &form.slug {
        match tab.items.iter().find(|item| &item.slug == slug) {
            Some(project) => (project.path.clone(), false),
            None => return Out::Toast(format!("{slug}: gone")),
        }
    } else {
        let Some(repo) = tab.repo().cloned() else {
            return Out::None;
        };

        match projects::create_in(&projects::dir(&repo), &title) {
            Ok(created) => (created.path, true),
            Err(error) => return Out::Toast(error),
        }
    };

    let old = std::fs::read_to_string(&path).unwrap_or_default();

    if let Err(error) = crate::engine::write_atomic(&path, projects::merge(&old, &title, &body).as_bytes()) {
        return Out::Toast(format!("{}: {error}", path.display()));
    }

    let slug = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();

    tab.form = None;
    tab.esc_at = None;
    tab.reload();
    tab.cursor = tab.items.iter().position(|item| item.slug == slug).unwrap_or(0);
    tab.focus = Focus::List;

    Out::Toast(if made {
        format!("project {slug} made")
    } else {
        format!("{slug}: saved")
    })
}

fn delete_projects_tab(tab: &mut ProjectsTab) -> Out {
    let Some(project) = tab.items.get(tab.cursor).cloned() else {
        return Out::None;
    };

    if let Err(error) = std::fs::remove_file(&project.path) {
        return Out::Toast(format!("{}: {error}", project.path.display()));
    }

    tab.reload();
    let thread_count = tab.thread_count(&project.slug);

    Out::Toast(if thread_count > 0 {
        format!(
            "{} deleted · {thread_count} thread(s) keep its name until edited",
            project.slug
        )
    } else {
        format!("{} deleted", project.slug)
    })
}

fn hints_projects_tab(tab: &ProjectsTab) -> &'static [(&'static str, &'static str)] {
    if let Some(form) = &tab.form {
        if form.slug.is_none() {
            &[("tab", "field"), ("ctrl+s", "make"), ("esc", "cancel")]
        } else {
            &[("tab", "field"), ("ctrl+s", "save"), ("esc", "close")]
        }
    } else if tab.del {
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
