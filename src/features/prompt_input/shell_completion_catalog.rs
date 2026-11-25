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

/// Times each command starts a line of THREAD (the thread's `!commands`) and the history.
pub(super) fn uses(thread: &[String], shell: &mut ShellComp) -> Arc<HashMap<String, usize>> {
    let hist = history(shell);
    let key = (thread.len(), shell.catalog.history.is_some());

    if let Some((key_value, count)) = &shell.uses
        && *key_value == key
    {
        return count.clone();
    }

    let mut count_value: HashMap<String, usize> = HashMap::new();

    for character in thread.iter().chain(hist.iter()) {
        if let Some(width) = character.split_whitespace().next() {
            *count_value.entry(width.to_string()).or_default() += 1;
        }
    }

    let count_2 = Arc::new(count_value);

    shell.uses = Some((key, count_2.clone()));

    count_2
}

/// The rows for CTX. None: nothing to offer.
pub(super) fn entries(ctx: &Ctx, dir: &Path, thread: &[String], shell: &mut ShellComp) -> Vec<Entry> {
    let word = ctx.word.as_str();
    let lower = word.to_lowercase();
    let mut out: Vec<Entry> = Vec::new();
    let tasks = shell.tasks(dir);
    let first = ctx.before.is_empty();
    let runner = runner_of(&ctx.before);

    // project tasks: by prefix, then those that hold the word
    let ranked = |hit: &dyn Fn(&Task) -> (bool, bool)| -> Vec<&Task> {
        let mut agent: Vec<&Task> = tasks.iter().filter(|thread_value| hit(thread_value).0).collect();

        agent.extend(tasks.iter().filter(|thread_2| !hit(thread_2).0 && hit(thread_2).1));
        agent
    };

    if first {
        let hit = |thread_3: &Task| {
            (
                thread_3.name.to_lowercase().starts_with(&lower) || thread_3.run.starts_with(word),
                thread_3.name.to_lowercase().contains(&lower),
            )
        };

        out.extend(ranked(&hit).into_iter().map(|thread_4| Entry {
            text: done(&thread_4.run),
            detail: thread_4.doc.clone(),
        }));
    } else if let Some(result) = runner {
        let hit = |thread_5: &Task| {
            let own = own(thread_5, result);

            (
                own && thread_5.name.to_lowercase().starts_with(&lower),
                own && thread_5.name.to_lowercase().contains(&lower),
            )
        };

        out.extend(ranked(&hit).into_iter().map(|thread_6| Entry {
            text: done(&thread_6.name),
            detail: thread_6.doc.clone(),
        }));
    }

    // commands and arguments: fish (the 1st word: ≥ 1 char)
    if !(matches!(runner, Some("just" | "make")) || first && word.is_empty()) {
        let mut rows: Vec<(String, String)> = Vec::new();

        append_fish_entries(ctx, dir, shell, word, first, &mut rows);

        if first {
            let uses = self::uses(thread, shell);

            // used commands first (most used first), then fish's order
            let mut idx: Vec<usize> = (0..rows.len()).collect();

            idx.sort_by_key(|&i| std::cmp::Reverse(uses.get(&rows[i].0).copied().unwrap_or(0)));
            rows = idx.into_iter().map(|i| rows[i].clone()).collect();
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
        if let Some(row) = shell.fish_rows.get(&ctx.line) {
            rows.clone_from(row);
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
            .flat_map(|character| character.iter())
            .filter(|character_value| character_value.starts_with(word))
            .take(MAX)
            .map(|character_2| (character_2.clone(), String::new()))
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
    if !out.iter().any(|error| error.text == text) {
        out.push(Entry { text, detail });
    }
}

#[cfg(test)]
pub(super) fn texts(ta: &TextArea, dir: &Path, shell: &mut ShellComp) -> Vec<String> {
    shell
        .open(ta, dir, &[])
        .map(|(_, error)| error.into_iter().map(|error_value| error_value.text).collect())
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
