#[cfg(test)]
use super::TextArea;
use super::{Arc, Ctx, Entry, HashMap, MAX, Path, Req, ShellComp, Task, done, own, runner_of};

/// The history, loaded in the background on the 1st call.
pub(super) fn history(shell: &mut ShellComp) -> Arc<Vec<String>> {
    if shell.catalog.history.is_none() && !shell.catalog.asked_history {
        shell.catalog.asked_history = true;
        shell.ask(Req::History);
        shell.poll();
    }

    shell.catalog.history.clone().unwrap_or_default()
}

/// Times each command starts a line of the thread's `!commands` and the history.
pub(super) fn uses(thread_commands: &[String], shell: &mut ShellComp) -> Arc<HashMap<String, usize>> {
    let hist = history(shell);
    let key = (thread_commands.len(), shell.catalog.history.is_some());

    if let Some((cached_key, cached)) = &shell.uses
        && *cached_key == key
    {
        return cached.clone();
    }

    let mut counts: HashMap<String, usize> = HashMap::new();

    for command in thread_commands.iter().chain(hist.iter()) {
        if let Some(name) = command.split_whitespace().next() {
            *counts.entry(name.to_string()).or_default() += 1;
        }
    }

    let counts = Arc::new(counts);

    shell.uses = Some((key, counts.clone()));

    counts
}

/// The rows for CTX. None: nothing to offer.
pub(super) fn entries(ctx: &Ctx, dir: &Path, thread_commands: &[String], shell: &mut ShellComp) -> Vec<Entry> {
    let word = ctx.word.as_str();
    let lower = word.to_lowercase();
    let mut out: Vec<Entry> = Vec::new();
    let tasks = shell.tasks(dir);
    let first = ctx.before.is_empty();
    let runner = runner_of(&ctx.before);

    // project tasks: by prefix, then those that hold the word
    let ranked = |hit: &dyn Fn(&Task) -> (bool, bool)| -> Vec<&Task> {
        let mut ranked_tasks: Vec<&Task> = tasks.iter().filter(|task| hit(task).0).collect();

        ranked_tasks.extend(tasks.iter().filter(|task| !hit(task).0 && hit(task).1));
        ranked_tasks
    };

    if first {
        let hit = |task: &Task| {
            (
                task.name.to_lowercase().starts_with(&lower) || task.run.starts_with(word),
                task.name.to_lowercase().contains(&lower),
            )
        };

        out.extend(ranked(&hit).into_iter().map(|task| Entry {
            text: done(&task.run),
            detail: task.doc.clone(),
        }));
    } else if let Some(runner_name) = runner {
        let hit = |task: &Task| {
            let own = own(task, runner_name);

            (
                own && task.name.to_lowercase().starts_with(&lower),
                own && task.name.to_lowercase().contains(&lower),
            )
        };

        out.extend(ranked(&hit).into_iter().map(|task| Entry {
            text: done(&task.name),
            detail: task.doc.clone(),
        }));
    }

    // commands and arguments: fish (the 1st word: ≥ 1 char)
    if !(matches!(runner, Some("just" | "make")) || first && word.is_empty()) {
        let mut rows: Vec<(String, String)> = Vec::new();

        append_fish_entries(ctx, dir, shell, word, first, &mut rows);

        if first {
            let uses = self::uses(thread_commands, shell);

            // used commands first (most used first), then fish's order
            let mut order: Vec<usize> = (0..rows.len()).collect();

            order.sort_by_key(|&i| std::cmp::Reverse(uses.get(&rows[i].0).copied().unwrap_or(0)));
            rows = order.into_iter().map(|i| rows[i].clone()).collect();
        }

        for (text, detail) in rows {
            let text = done(&text);

            append_unique_entry(&mut out, detail, text);
        }
    }

    out.truncate(MAX);

    out
}

fn append_fish_entries(
    ctx: &Ctx,
    dir: &Path,
    shell: &mut ShellComp,
    word: &str,
    first: bool,
    rows: &mut Vec<(String, String)>,
) {
    if shell.fish {
        if let Some(cached) = shell.fish_rows.get(&ctx.line) {
            rows.clone_from(cached);
        } else {
            request_fish_rows(shell, ctx, dir);

            *rows = shell
                .fish_rows
                .get(&ctx.line)
                .cloned()
                .unwrap_or_else(|| shell.stale(ctx));
        }
    } else if first {
        request_command_catalog(shell);

        *rows = shell
            .catalog
            .commands
            .iter()
            .flat_map(|commands| commands.iter())
            .filter(|command| command.starts_with(word))
            .take(MAX)
            .map(|command| (command.clone(), String::new()))
            .collect();
    }
}

fn request_command_catalog(shell: &mut ShellComp) {
    if shell.catalog.commands.is_none() && !shell.catalog.asked_commands {
        shell.catalog.asked_commands = true;
        shell.ask(Req::Commands);
        shell.poll();
    }
}

pub(super) fn append_unique_entry(out: &mut Vec<Entry>, detail: String, text: String) {
    if !out.iter().any(|entry| entry.text == text) {
        out.push(Entry { text, detail });
    }
}

#[cfg(test)]
pub(super) fn texts(ta: &TextArea, dir: &Path, shell: &mut ShellComp) -> Vec<String> {
    shell
        .open(ta, dir, &[])
        .map(|(_, entries)| entries.into_iter().map(|entry| entry.text).collect())
        .unwrap_or_default()
}

fn request_fish_rows(shell: &mut ShellComp, ctx: &Ctx, dir: &Path) {
    if shell.asked == ctx.line {
        return;
    }

    shell.asked.clone_from(&ctx.line);
    shell.ask(Req::Fish(ctx.line.clone(), dir.to_path_buf()));
    shell.poll();
}
