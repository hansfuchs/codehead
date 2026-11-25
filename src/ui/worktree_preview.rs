use crate::domain::threads::{Worktree, delete_thread, drop_worktree, git, repo_of, unpushed};
use crate::ui::theme;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use std::path::Path;
pub(crate) fn preview_header(worktree: &Worktree) -> Vec<Line<'static>> {
    let title = worktree
        .thread
        .as_ref()
        .map_or_else(|| worktree.branch.clone(), |thread| thread.title.clone());
    let mut out = vec![
        Line::styled(title, theme::accent_bold()),
        Line::styled(worktree.path.clone(), theme::muted()),
        Line::raw(""),
    ];

    match &worktree.thread {
        Some(thread) => {
            preview_thread(thread, &mut out);
        }
        None => out.push(Line::styled("thread: none", theme::muted())),
    }

    out.push(Line::raw(""));
    out
}

pub(crate) fn preview(worktree: &Worktree) -> Vec<Line<'static>> {
    let dir = Path::new(&worktree.path);
    let mut out = preview_header(worktree);

    if !worktree.linked {
        out.push(Line::styled("works in place: 0 worktree of its own", theme::muted()));

        if !dir.join(".git").exists() && repo_of(dir).is_none() {
            return out;
        }
    }

    let (_, st, _) = git(dir, &["status", "--porcelain"]);
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

    let rev = if !worktree.linked || worktree.branch == "(detached)" {
        "HEAD".to_string()
    } else {
        worktree.branch.clone()
    };

    let (_, log, _) = git(dir, &["log", "--oneline", "-15", &rev, "--not", "--remotes"]);
    let log: Vec<&str> = log.lines().collect();

    out.push(Line::raw(""));
    out.push(Line::styled(
        format!("unpushed commits: {}", log.len()),
        col(!log.is_empty()),
    ));
    out.extend(log.iter().map(|line| Line::raw(format!("  {line}"))));
    let (_, last, _) = git(dir, &["log", "-1", "--format=%cr · %s"]);

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

    for worktree in items {
        remove_worktree(repo, logs, &mut out, danger, warn, worktree);
    }

    out
}

fn remove_worktree(
    repo: &Path,
    logs: &Path,
    out: &mut Vec<Line<'static>>,
    danger: Style,
    warn: Style,
    worktree: &Worktree,
) {
    let dir = Path::new(&worktree.path);

    out.push(Line::from(vec![
        Span::styled(worktree.label(), theme::bold()),
        Span::styled(format!("  {}", worktree.path), theme::muted()),
    ]));

    if !worktree.linked {
        // in place: the dir is not ours. Delete the thread only.
        delete_in_place(worktree, out);

        return;
    }

    if !dir.exists() {
        out.push(Line::styled("  skip: worktree gone", theme::muted()));
        return;
    }

    let repo = repo_of(dir).unwrap_or_else(|| repo.to_path_buf());
    let repo = repo.as_path();

    teardown(worktree, out, warn);

    if !stash_changes(worktree, out, warn, danger) {
        return;
    }

    if let Some(thread) = &worktree.thread {
        let ok = delete_thread(&thread.id);
        let short: String = thread.id.chars().take(12).collect();

        out.push(Line::raw(format!(
            "  thread {short}: {}",
            if ok { "deleted" } else { "delete failed" }
        )));
        let _ = std::fs::remove_file(logs.join(format!("host-{}.log", thread.id)));
    }

    let up = unpushed(&worktree.branch, dir);

    if worktree.branch == "(detached)" && up > 0 {
        out.push(Line::styled(
            format!("  skip: detached HEAD with {up} unpushed commits. commits would be lost."),
            danger,
        ));
        return;
    }

    if let Err(error) = drop_worktree(repo, dir) {
        out.push(Line::styled(format!("  worktree: {error}"), danger));
        return;
    }

    out.push(Line::raw("  worktree removed"));

    if worktree.branch == "(detached)" {
        return;
    }

    // the worktree's own repo (can differ from the repo codehead runs in)

    // the teardown script (docker down, test DB …) before the worktree goes

    remove_branch(repo, &worktree.branch, up, out, warn, danger);
}

use crate::engine::store;

fn preview_thread(thread: &crate::domain::threads::Thread, out: &mut Vec<Line<'static>>) {
    out.push(Line::from(vec![
        Span::styled("thread ", theme::muted()),
        Span::raw(thread.id.chars().take(12).collect::<String>()),
        Span::styled(" · state ", theme::muted()),
        Span::raw(thread.state.clone()),
        Span::styled(" · live sessions ", theme::muted()),
        Span::raw(thread.live.to_string()),
    ]));

    for line in thread.title.lines() {
        out.push(Line::raw(line.to_string()));
    }

    if let Some(needs) = &thread.needs {
        out.push(Line::styled(
            format!("needs you: {needs}"),
            theme::fg(theme::pal().attention()),
        ));
    }

    preview_agents(thread, out);
    preview_facts(thread, out);
    preview_task(thread, out);
}

fn preview_agents(thread: &crate::domain::threads::Thread, out: &mut Vec<Line<'static>>) {
    // agents: the last status each reported
    if !thread.agents.is_empty() {
        preview_agent_notes(thread, out);
    }
}

fn preview_agent_notes(thread: &crate::domain::threads::Thread, out: &mut Vec<Line<'static>>) {
    out.push(Line::raw(""));

    for agent in &thread.agents {
        preview_agent_note(thread, out, agent);
    }
}

fn preview_agent_note(thread: &crate::domain::threads::Thread, out: &mut Vec<Line<'static>>, agent: &String) {
    let note = thread
        .notes
        .iter()
        .find(|note| note.0 == *agent)
        .map(|note| {
            if note.2.is_empty() {
                note.1.clone()
            } else {
                format!("{} · {}", note.1, note.2)
            }
        })
        .unwrap_or_default();

    out.push(Line::from(vec![
        Span::styled(format!("{agent:<10}"), theme::bold()),
        Span::styled(note, theme::muted()),
    ]));
}

fn preview_facts(thread: &crate::domain::threads::Thread, out: &mut Vec<Line<'static>>) {
    let mut facts: Vec<String> = Vec::new();

    if let Some((files, add, del)) = thread.stat.filter(|stat| stat.0 > 0) {
        facts.push(format!("changes {files} files +{add} −{del}"));
    }

    if let Some(port) = thread.port {
        facts.push(format!(
            "ports {port}-{}",
            port + crate::infra::worktree_setup::PORTS - 1
        ));
    }

    if let Some(setup) = &thread.setup {
        facts.push(format!("setup {setup}"));
    }

    for url in &thread.urls {
        facts.push(format!("↗ {}", url.trim_start_matches("https://")));
    }

    if let Some(parent) = &thread.parent {
        facts.push(format!("continues {}", parent.chars().take(12).collect::<String>()));
    }

    if !facts.is_empty() {
        out.push(Line::raw(""));
        out.push(Line::styled(facts.join(" · "), theme::muted()));
    }
}

fn preview_task(thread: &crate::domain::threads::Thread, out: &mut Vec<Line<'static>>) {
    // the task: its 1st lines
    let task: Vec<&str> = thread
        .task
        .lines()
        .filter(|line| !line.trim().is_empty())
        .take(5)
        .collect();

    if !task.is_empty() {
        out.push(Line::raw(""));
        out.push(Line::styled("task", theme::muted()));
        out.extend(
            task.iter()
                .map(|line| Line::raw(line.chars().take(200).collect::<String>())),
        );
    }
}

fn teardown(worktree: &Worktree, out: &mut Vec<Line<'static>>, warn: Style) {
    if let Some(thread) = &worktree.thread
        && let Some(st) = store::load(&thread.id)
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

fn stash_changes(worktree: &Worktree, out: &mut Vec<Line<'static>>, warn: Style, danger: Style) -> bool {
    let dir = Path::new(&worktree.path);
    let (_, dirty, _) = git(dir, &["status", "--porcelain"]);
    let n_dirty = dirty.lines().count();

    if n_dirty > 0 {
        let msg = format!("codehead: {} before worktree removal", worktree.branch);
        let (ok, _, err) = git(dir, &["stash", "push", "--include-untracked", "-m", &msg]);

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

fn delete_in_place(worktree: &Worktree, out: &mut Vec<Line<'static>>) {
    if let Some(thread) = &worktree.thread {
        let ok = delete_thread(&thread.id);

        out.push(Line::raw(format!(
            "  thread {}: {} · dir kept (in place)",
            thread.id.chars().take(12).collect::<String>(),
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
