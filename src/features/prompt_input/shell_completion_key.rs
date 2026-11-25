use super::{KeyCode, KeyEvent, KeyModifiers, Path, ShellComp, TextArea, context};

impl ShellComp {
    /// Key K (shell mode's prompt TA, DIR its dir, THREAD the thread's `!commands`). True: used.
    pub(crate) fn key(&mut self, key_value: KeyEvent, ta: &mut TextArea, dir: &Path, thread: &[String]) -> bool {
        key_shell_comp(self, key_value, ta, dir, thread)
    }
}

fn open_forced_completion(
    ta: &mut TextArea,
    dir: &Path,
    thread: &[String],
    shell: &mut ShellComp,
) -> Result<Option<(super::Ctx, Vec<super::Entry>)>, bool> {
    let Some(ctx) = context(ta) else { return Err(false) };

    shell.closed = None;
    shell.forced = Some(ctx.at);
    Ok(match shell.open(ta, dir, thread) {
        Some(output_value) => Some(output_value),
        // fish still at work: taken, its rows show when in (0 rows: the key goes on)
        None => return Err(!(ctx.before.is_empty() && ctx.word.is_empty()) && shell.fish),
    })
}

fn key_shell_comp(
    shell: &mut ShellComp,
    key_value: KeyEvent,
    ta: &mut TextArea,
    dir: &Path,
    thread: &[String],
) -> bool {
    if !key_value.modifiers.is_empty() && key_value.modifiers != KeyModifiers::SHIFT {
        return false;
    }

    let open = match shell.open(ta, dir, thread) {
        Some(output) => Some(output),
        // tab: open for this word
        None if key_value.code == KeyCode::Tab => match open_forced_completion(ta, dir, thread, shell) {
            Ok(value) => value,
            Err(value) => return value,
        },
        None => None,
    };

    let Some((ctx, entries)) = open else { return false };
    let count = entries.len();

    let show = |me: &mut ShellComp, ta: &mut TextArea| {
        let text = entries[me.cursor].text.trim_end().to_string();

        me.preview.show(ta, ctx.at, &ctx.word, &text);
    };

    match (key_value.code, shell.active) {
        // passive: tab takes the 1 row, else makes the dropdown active on its 1st row
        (KeyCode::Tab, false) if count == 1 => shell.pick(ta, &ctx, &entries[0]),
        (KeyCode::Tab, false) => {
            shell.active = true;
            shell.cursor = 0;
            show(shell, ta);
        }
        (KeyCode::Up | KeyCode::BackTab, true) => {
            shell.cursor = (shell.cursor + count - 1) % count;
            show(shell, ta);
        }
        (KeyCode::Down | KeyCode::Tab, true) => {
            shell.cursor = (shell.cursor + 1) % count;
            show(shell, ta);
        }
        (KeyCode::Enter, true) => shell.pick(ta, &ctx, &entries[shell.cursor]),
        (KeyCode::Esc, _) => {
            shell.preview.revert(ta);
            shell.closed = Some(ctx.at);
        }
        // a name char: the typed word again (the filter), the char after it
        (KeyCode::Char(character), true) if !character.is_whitespace() && shell.preview.revert(ta) => {
            return false;
        }
        _ => return false,
    }

    true
}
