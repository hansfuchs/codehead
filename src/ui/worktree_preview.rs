use crate::domain::threads::{Worktree, delete_thread, drop_worktree, git, repo_of, unpushed};
use crate::ui::theme;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use std::path::Path;
pub(crate) fn preview_header(width: &Worktree) -> Vec<Line<'static>> {
    let title = width
        .thread
        .as_ref()
        .map_or_else(|| width.branch.clone(), |thread_value| thread_value.title.clone());
    let mut out = vec![
        Line::styled(title, theme::accent_bold()),
        Line::styled(width.path.clone(), theme::muted()),
        Line::raw(""),
    ];

    match &width.thread {
        Some(thread_2) => {
            preview_thread(thread_2, &mut out);
        }
        None => out.push(Line::styled("thread: none", theme::muted())),
    }

    out.push(Line::raw(""));
    out
}

pub(crate) fn preview(width: &Worktree) -> Vec<Line<'static>> {
    let path_value = Path::new(&width.path);
    let mut out = preview_header(width);

    if !width.linked {
        out.push(Line::styled("works in place: 0 worktree of its own", theme::muted()));

        if !path_value.join(".git").exists() && repo_of(path_value).is_none() {
            return out;
        }
    }

    let (_, st, _) = git(path_value, &["status", "--porcelain"]);
    let st: Vec<&str> = st.lines().collect();

    let col = |bad: bool| {
        Style::default().fg(if bad {
            theme::pal().error()
        } else {
            theme::pal().success()
        })
    };

    out.push(Line::styled(
        format!("uncommitted: {} files", st.len()),
        col(!st.is_empty()),
    ));
    out.extend(st.iter().take(15).map(|line| Line::raw(format!("  {line}"))));

    let result = if !width.linked || width.branch == "(detached)" {
        "HEAD".to_string()
    } else {
        width.branch.clone()
    };

    let (_, log, _) = git(path_value, &["log", "--oneline", "-15", &result, "--not", "--remotes"]);
    let log: Vec<&str> = log.lines().collect();

    out.push(Line::raw(""));
    out.push(Line::styled(
        format!("unpushed commits: {}", log.len()),
        col(!log.is_empty()),
    ));
    out.extend(log.iter().map(|line_value| Line::raw(format!("  {line_value}"))));
    let (_, last, _) = git(path_value, &["log", "-1", "--format=%cr · %s"]);

    out.push(Line::raw(""));
    out.push(Line::from(vec![
        Span::styled("last commit: ", theme::muted()),
        Span::raw(last.trim().to_string()),
    ]));

    out
}

/// Delete thread, remove worktree (uncommitted files → git stash), delete branch with 0 unpushed commits.
pub(crate) fn remove(repo: &Path, items: &[Worktree], logs: &Path) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let danger = Style::default().fg(theme::pal().error());
    let warn = theme::fg(theme::pal().caution());

    for width in items {
        remove_path_value(repo, logs, &mut out, danger, warn, width);
    }

    out
}

fn remove_path_value(
    repo: &Path,
    logs: &Path,
    out: &mut Vec<Line<'static>>,
    danger: Style,
    warn: Style,
    width: &Worktree,
) {
    let path_value = Path::new(&width.path);

    out.push(Line::from(vec![
        Span::styled(width.label(), theme::bold()),
        Span::styled(format!("  {}", width.path), theme::muted()),
    ]));

    if !width.linked {
        // in place: the dir is not ours. Delete the thread only.
        delete_in_place(width, out);

        return;
    }

    if !path_value.exists() {
        out.push(Line::styled("  skip: worktree gone", theme::muted()));
        return;
    }

    let repo = repo_of(path_value).unwrap_or_else(|| repo.to_path_buf());
    let repo = repo.as_path();

    teardown(width, out, warn);

    if !stash_changes(width, out, warn, danger) {
        return;
    }

    if let Some(thread_3) = &width.thread {
        let ok = delete_thread(&thread_3.id);
        let short: String = thread_3.id.chars().take(12).collect();

        out.push(Line::raw(format!(
            "  thread {short}: {}",
            if ok { "deleted" } else { "delete failed" }
        )));
        let _ = std::fs::remove_file(logs.join(format!("host-{}.log", thread_3.id)));
    }

    let up = unpushed(&width.branch, path_value);

    if width.branch == "(detached)" && up > 0 {
        out.push(Line::styled(
            format!("  skip: detached HEAD with {up} unpushed commits. commits would be lost."),
            danger,
        ));
        return;
    }

    if let Err(error_value) = drop_worktree(repo, path_value) {
        out.push(Line::styled(format!("  worktree: {error_value}"), danger));
        return;
    }

    out.push(Line::raw("  worktree removed"));

    if width.branch == "(detached)" {
        return;
    }

    // the worktree's own repo (can differ from the repo codehead runs in)

    // the teardown script (docker down, test DB …) before the worktree goes

    remove_branch(repo, &width.branch, up, out, warn, danger);
}

use crate::engine::store;

fn preview_thread(thread_2: &crate::domain::threads::Thread, out: &mut Vec<Line<'static>>) {
    out.push(Line::from(vec![
        Span::styled("thread ", theme::muted()),
        Span::raw(thread_2.id.chars().take(12).collect::<String>()),
        Span::styled(" · state ", theme::muted()),
        Span::raw(thread_2.state.clone()),
        Span::styled(" · live sessions ", theme::muted()),
        Span::raw(thread_2.live.to_string()),
    ]));

    for line in thread_2.title.lines() {
        out.push(Line::raw(line.to_string()));
    }

    if let Some(count) = &thread_2.needs {
        out.push(Line::styled(
            format!("needs you: {count}"),
            theme::fg(theme::pal().attention()),
        ));
    }

    preview_agents(thread_2, out);
    preview_facts(thread_2, out);
    preview_task(thread_2, out);
}

fn preview_agents(thread_2: &crate::domain::threads::Thread, out: &mut Vec<Line<'static>>) {
    // agents: the last status each reported
    if !thread_2.agents.is_empty() {
        preview_agents_note(thread_2, out);
    }
}

fn preview_agents_note(thread_2: &crate::domain::threads::Thread, out: &mut Vec<Line<'static>>) {
    out.push(Line::raw(""));

    for agent in &thread_2.agents {
        preview_agents_note_note(thread_2, out, agent);
    }
}

fn preview_agents_note_note(thread_2: &crate::domain::threads::Thread, out: &mut Vec<Line<'static>>, agent: &String) {
    let note = thread_2
        .notes
        .iter()
        .find(|x| x.0 == *agent)
        .map(|x| {
            if x.2.is_empty() {
                x.1.clone()
            } else {
                format!("{} · {}", x.1, x.2)
            }
        })
        .unwrap_or_default();

    out.push(Line::from(vec![
        Span::styled(format!("{agent:<10}"), theme::bold()),
        Span::styled(note, theme::muted()),
    ]));
}

fn preview_facts(thread_2: &crate::domain::threads::Thread, out: &mut Vec<Line<'static>>) {
    let mut facts: Vec<String> = Vec::new();

    if let Some((files, add, del)) = thread_2.stat.filter(|text| text.0 > 0) {
        facts.push(format!("changes {files} files +{add} −{del}"));
    }

    if let Some(path_value) = thread_2.port {
        facts.push(format!(
            "ports {path_value}-{}",
            path_value + crate::infra::worktree_setup::PORTS - 1
        ));
    }

    if let Some(text_value) = &thread_2.setup {
        facts.push(format!("setup {text_value}"));
    }

    for user in &thread_2.urls {
        facts.push(format!("↗ {}", user.trim_start_matches("https://")));
    }

    if let Some(path_2) = &thread_2.parent {
        facts.push(format!("continues {}", path_2.chars().take(12).collect::<String>()));
    }

    if !facts.is_empty() {
        out.push(Line::raw(""));
        out.push(Line::styled(facts.join(" · "), theme::muted()));
    }
}

fn preview_task(thread_2: &crate::domain::threads::Thread, out: &mut Vec<Line<'static>>) {
    // the task: its 1st lines
    let task: Vec<&str> = thread_2
        .task
        .lines()
        .filter(|line_value| !line_value.trim().is_empty())
        .take(5)
        .collect();

    if !task.is_empty() {
        out.push(Line::raw(""));
        out.push(Line::styled("task", theme::muted()));
        out.extend(
            task.iter()
                .map(|line_2| Line::raw(line_2.chars().take(200).collect::<String>())),
        );
    }
}

fn teardown(width: &Worktree, out: &mut Vec<Line<'static>>, warn: Style) {
    if let Some(thread_2) = &width.thread
        && let Some(st) = store::load(&thread_2.id)
        && let Some((label, ok, output)) = crate::infra::worktree_setup::run_teardown(&st)
    {
        let st = if ok { theme::muted() } else { warn };

        out.push(Line::styled(
            format!("  {label}: {}", if ok { "ran" } else { "failed" }),
            st,
        ));

        if !ok {
            out.extend(
                output
                    .lines()
                    .rev()
                    .take(3)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .map(|line| Line::styled(format!("    {line}"), theme::muted())),
            );
        }
    }
}

fn stash_changes(width: &Worktree, out: &mut Vec<Line<'static>>, warn: Style, danger: Style) -> bool {
    let path_value = Path::new(&width.path);
    let (_, dirty, _) = git(path_value, &["status", "--porcelain"]);
    let n_dirty = dirty.lines().count();

    if n_dirty > 0 {
        let msg = format!("codehead: {} before worktree removal", width.branch);
        let (ok, _, err) = git(path_value, &["stash", "push", "--include-untracked", "-m", &msg]);

        if !ok {
            out.push(Line::styled(format!("  skip: stash failed: {err}"), danger));
            return false;
        }

        out.push(Line::styled(
            format!("  {n_dirty} uncommitted files stashed: \"{msg}\""),
            warn,
        ));
    }

    true
}

fn delete_in_place(width: &Worktree, out: &mut Vec<Line<'static>>) {
    if let Some(thread_value) = &width.thread {
        let ok = delete_thread(&thread_value.id);

        out.push(Line::raw(format!(
            "  thread {}: {} · dir kept (in place)",
            thread_value.id.chars().take(12).collect::<String>(),
            if ok { "deleted" } else { "delete failed" }
        )));
    }
}

fn remove_branch(repo: &Path, branch: &str, up: usize, out: &mut Vec<Line<'static>>, warn: Style, danger: Style) {
    if up > 0 {
        out.push(Line::styled(format!("  branch kept: {up} unpushed commits"), warn));
    } else {
        let (ok, _, err) = git(repo, &["branch", "-D", branch]);

        out.push(if ok {
            Line::raw("  branch deleted")
        } else {
            Line::styled(format!("  branch: {err}"), danger)
        });
    }
}
