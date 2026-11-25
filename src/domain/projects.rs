//! Projects: 1 brief (`<slug>.md`) per project, per repo. A thread belongs to 0..n projects of
//! its repo (`store::Thread::projects`); the thread lists filter by them, the agents read the
//! briefs. The owner makes projects (projects tab); an agent only assigns existing ones.
//!
//! Where: the home repo, the repo's dir there (the `Notes:` dir of the repo's line in the home
//! repo's AGENTS.md, else `<home>/<repo name>/`), its `projects/`. 0 home repo:
//! `<data dir>/projects/<repo name>/`.
//!
//! A brief: an optional front matter (`title:`, `done: true`), then free text (goal, decisions,
//! open points). 0 title: the slug.
//!
//! Naming (the agents' `set_title`): title `area: outcome`, branch `<kind>/<area>-<slug>`. Kind
//! and area of a thread: from its branch / title (thread list filters `kind:fix`, `area:menu`).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Project {
    pub slug: String,
    pub title: String,
    pub done: bool,
    /// The brief's 1st line of text (after the front matter, 0 heading marks).
    pub goal: String,
    pub path: PathBuf,
}

/// The projects dir of REPO (a main checkout).
pub(crate) fn dir(repo: &Path) -> PathBuf {
    let home = if cfg!(test) {
        None
    } else {
        crate::infra::config::home_repo()
    };

    dir_in(home.as_deref(), &crate::engine::data_dir(), repo)
}

/// The projects dir of REPO with home repo HOME, data dir DATA.
pub(crate) fn dir_in(home: Option<&Path>, data: &Path, repo: &Path) -> PathBuf {
    let name = repo_name(repo);

    match home {
        Some(home) => {
            let agents = std::fs::read_to_string(home.join("AGENTS.md")).unwrap_or_default();
            let sub = crate::infra::home::notes_of(&agents, repo, &name).unwrap_or_else(|| name.clone());

            home.join(sub.trim_end_matches('/')).join("projects")
        }
        None => data.join("projects").join(name),
    }
}

fn repo_name(repo: &Path) -> String {
    repo.file_name()
        .map_or_else(|| "repo".into(), |name| name.to_string_lossy().into_owned())
}

/// The projects of REPO: active first, then A→Z.
pub(crate) fn list(repo: &Path) -> Vec<Project> {
    list_in(&dir(repo))
}

pub(crate) fn list_in(dir: &Path) -> Vec<Project> {
    let mut projects: Vec<Project> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "md"))
        .filter_map(|path| read(&path))
        .collect();

    projects.sort_by(|left, right| {
        left.done
            .cmp(&right.done)
            .then_with(|| left.title.to_lowercase().cmp(&right.title.to_lowercase()))
    });

    projects
}

/// The brief at PATH.
pub(crate) fn read(path: &Path) -> Option<Project> {
    let slug = path.file_stem()?.to_string_lossy().into_owned();
    let text = std::fs::read_to_string(path).ok()?;
    let (title, done, goal) = parse(&text);

    Some(Project {
        title: title.unwrap_or_else(|| slug.clone()),
        slug,
        done,
        goal,
        path: path.to_path_buf(),
    })
}

/// Title, done, goal of a brief's TEXT.
pub(crate) fn parse(text: &str) -> (Option<String>, bool, String) {
    let (front, body) = split_front(text);
    let mut title = None;
    let mut done = false;

    for line in front.lines() {
        read_project_field(&mut title, &mut done, line);
    }

    let goal = body
        .lines()
        .map(|line| line.trim().trim_start_matches('#').trim())
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .to_string();

    (title, done, goal)
}

fn read_project_field(title: &mut Option<String>, done: &mut bool, line: &str) {
    if let Some((key, value)) = line.split_once(':') {
        let value = value.trim().trim_matches('"').trim_matches('\'');

        match key.trim() {
            "title" if !value.is_empty() => *title = Some(value.to_string()),
            "done" => *done = value == "true" || value == "yes",
            _ => {}
        }
    }
}

/// (front matter, body) of TEXT: 0 front matter → ("", TEXT).
fn split_front(text: &str) -> (&str, &str) {
    let Some(rest) = text.strip_prefix("---\n").or_else(|| text.strip_prefix("---\r\n")) else {
        return ("", text);
    };

    match rest.find("\n---") {
        Some(i) => {
            let after = &rest[i + 4..];

            (
                &rest[..i],
                after
                    .strip_prefix('\n')
                    .or_else(|| after.strip_prefix("\r\n"))
                    .unwrap_or(after),
            )
        }
        None => ("", text),
    }
}

/// TEXT with front matter key K = V (None: K out). 0 front matter: a new one.
pub(crate) fn set_front(text: &str, key: &str, value: Option<&str>) -> String {
    let (front, body) = split_front(text);
    let mut lines: Vec<String> = front
        .lines()
        .filter(|line| line.split_once(':').is_none_or(|(name, _)| name.trim() != key))
        .map(String::from)
        .collect();

    if let Some(value) = value {
        lines.push(format!("{key}: {value}"));
    }

    if lines.is_empty() {
        return body.to_string();
    }

    format!("---\n{}\n---\n{body}", lines.join("\n"))
}

/// The brief body of TEXT: after the front matter, 0 blank lines on top.
pub(crate) fn body_of(text: &str) -> String {
    split_front(text).1.trim_start_matches(['\n', '\r']).to_string()
}

/// TEXT with title TITLE, body BODY: the front matter's other keys (`done:`) stay.
pub(crate) fn merge(text: &str, title: &str, body: &str) -> String {
    let (front, _) = split_front(text);
    let mut body = body.trim_start_matches(['\n', '\r']).to_string();

    if !body.ends_with('\n') {
        body.push('\n');
    }

    let base = if front.is_empty() {
        format!("\n{body}")
    } else {
        format!("---\n{front}\n---\n\n{body}")
    };

    set_front(&base, "title", Some(title.trim()))
}

/// The default body of a new brief.
pub(crate) const TEMPLATE: &str = "Goal: \n\n## Decisions\n\n## Open\n";

/// A new project TITLE in DIR. Err: 0 title, the slug exists, a write error.
pub(crate) fn create_in(dir: &Path, title: &str) -> Result<Project, String> {
    let title = title.trim();
    let slug = crate::domain::notes::slug(title);

    if slug.is_empty() {
        return Err("title: 1 letter or digit at least".into());
    }

    let path = dir.join(format!("{slug}.md"));

    if path.exists() {
        return Err(format!("{slug}: exists"));
    }

    let text = format!("---\ntitle: {title}\n---\n\n{TEMPLATE}");

    crate::engine::write_atomic(&path, text.as_bytes()).map_err(|error| format!("{}: {error}", path.display()))?;

    read(&path).ok_or_else(|| format!("{}: unreadable", path.display()))
}

/// Project P done / active again.
pub(crate) fn set_done(project: &Project, done: bool) -> Result<(), String> {
    let text =
        std::fs::read_to_string(&project.path).map_err(|error| format!("{}: {error}", project.path.display()))?;
    let text = set_front(&text, "done", done.then_some("true"));

    crate::engine::write_atomic(&project.path, text.as_bytes())
        .map_err(|error| format!("{}: {error}", project.path.display()))
}

/// The slugs of SLUGS that are projects of REPO (any state), in the order given, 0 dupes. Err:
/// the unknown ones, with the known active ones.
pub(crate) fn resolve(repo: &Path, slugs: &[String]) -> Result<Vec<String>, String> {
    resolve_in(&list(repo), slugs)
}

pub(crate) fn resolve_in(all: &[Project], slugs: &[String]) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    let mut bad = Vec::new();

    for slug in slugs {
        let wanted = slug.trim();

        match all
            .iter()
            .find(|project| project.slug == wanted || project.title.eq_ignore_ascii_case(wanted))
        {
            Some(project) if !out.contains(&project.slug) => out.push(project.slug.clone()),
            Some(_) => {}
            None => bad.push(wanted.to_string()),
        }
    }

    if bad.is_empty() {
        return Ok(out);
    }

    let known: Vec<&str> = all
        .iter()
        .filter(|project| !project.done)
        .map(|project| project.slug.as_str())
        .collect();

    Err(format!(
        "unknown project(s): {}. projects of this repo: {}. only the owner makes projects.",
        bad.join(", "),
        if known.is_empty() {
            "0".to_string()
        } else {
            known.join(", ")
        }
    ))
}

// ---------- naming ----------

/// Kinds of work: the branch prefix of an agent-named branch.
pub(crate) const KINDS: &[&str] = &[
    "feat", "fix", "refactor", "perf", "docs", "test", "chore", "style", "ci", "build",
];

/// Words that name no outcome: 0 title of only these.
const VAGUE: &[&str] = &[
    "misc",
    "misc.",
    "improvements",
    "improvement",
    "changes",
    "updates",
    "update",
    "fixes",
    "stuff",
    "wip",
    "work",
    "various",
    "tweaks",
    "cleanup",
    "more",
];

// ---------- the agents' tools ----------

/// The system prompt block on naming and projects for thread T.
pub(crate) fn prompt(thread: &crate::engine::store::Thread) -> String {
    let mut text = String::from(
        "\n\n# Naming\n\
Name the thread after what it does, with the `set_title` tool of the `codehead` MCP server: after your 1st turn's work, \
again when the work changes direction (not every turn). Title: `area: outcome`, imperative, e.g. `menu: live quick commands`; \
never vague (`misc. improvements #8`).",
    );

    if thread.worktree.is_some() {
        let _ = write!(
            text,
            " Branch (`branch` of `set_title`, this thread's worktree only): `<kind>/<area>-<slug>`, kind: {}, e.g. `feat/menu-live-commands`; \
codehead renames it, also on the remote.",
            KINDS.join(" ")
        );
    }

    if thread.owner_choices.title_set {
        text.push_str(" The owner wrote the title: it stays; `set_title` still names the branch.");
    }

    let Some(repo) = repo_of(thread) else { return text };
    let all = list(&repo);
    let active: Vec<&Project> = all.iter().filter(|project| !project.done).collect();

    if active.is_empty() && thread.projects.is_empty() {
        return text;
    }

    let _ = write!(
        text,
        "\n\n# Projects\nProjects of this repo (briefs in `{}`):",
        dir(&repo).display()
    );

    for project in &active {
        let _ = write!(
            text,
            "\n- `{}`: {}{}",
            project.slug,
            project.title,
            if project.goal.is_empty() {
                String::new()
            } else {
                format!(" ({})", project.goal)
            }
        );
    }

    if thread.projects.is_empty() {
        if !thread.owner_choices.projects_set {
            text.push_str("\nThis thread: 0 projects yet. Once its work is clear, assign the fitting ones (0..n) with `set_projects`. Never make a project: only the owner does.");
        }
    } else {
        let _ = write!(
            text,
            "\nThis thread: {}. Read their briefs before work.",
            thread.projects.join(", ")
        );

        if !thread.owner_choices.projects_set {
            text.push_str(" The work moved to other projects: `set_projects`.");
        }
    }

    text
}

#[cfg(test)]
#[path = "projects_tests.rs"]
mod tests;

#[path = "project_branches.rs"]
mod project_branches;
pub(crate) use project_branches::*;
