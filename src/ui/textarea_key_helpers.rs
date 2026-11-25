use super::{Instant, KeyCode, KeyEvent, KeyModifiers, Op, Outcome, Pos, SAME_PRESS, TextArea, clip, keylog, line_mod};

impl TextArea {
    pub(super) fn key_at(&mut self, key: KeyEvent, now: Instant) -> Outcome {
        keylog(&key, now);
        let space = key.code == KeyCode::Char(' ') && key.modifiers.difference(KeyModifiers::SHIFT).is_empty();

        // the 2nd space as a key right after the burst: the 2 spaces are in already
        if space
            && self
                .period_undone
                .take()
                .is_some_and(|thread| now - thread <= SAME_PRESS)
        {
            return Outcome::Handled;
        }

        self.period_undone = None;
        self.recent.push((now, key.code));

        if self.recent.len() > 5 {
            self.recent.remove(0);
        }

        let out = self.key_inner(key);

        if space {
            self.undo_auto_period(now);
        }

        out
    }

    pub(super) fn key_inner(&mut self, key: KeyEvent) -> Outcome {
        edit_key(key, self)
    }
}

fn cut_selection(widget_state: &mut TextArea) {
    if let Some(thread_value) = widget_state.selected_text() {
        clip::copy(&thread_value);
        widget_state.snapshot(Op::Other);
        widget_state.delete_selection();
    }
}

fn command_key(area: &mut TextArea, key: KeyEvent, character: char) -> Outcome {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let sup = key.modifiers.contains(KeyModifiers::SUPER);

    match character.to_ascii_lowercase() {
        'a' if shift || sup => area.select_all(),
        // ⌘← / ⌘→ in macOS terminals arrive as ctrl+a / ctrl+e (readline): row start / end
        'a' => area.line_edge(false, false),
        'c' => {
            if let Some(thread) = area.selected_text() {
                clip::copy(&thread);
            }
        }
        'x' => {
            cut_selection(area);
        }
        'v' => {
            if let Some(thread_2) = clip::paste() {
                area.insert(&thread_2);
            }
        }
        'z' if shift || character == 'Z' => area.restore(false),
        'z' => area.restore(true),
        'y' => area.restore(false),
        // readline keys: ctrl only (⌘ + these = 0 effect)
        'e' | 'w' | 'u' | 'k' | 'h' if !ctrl => return Outcome::Ignored,
        'e' => area.line_edge(true, false),
        // ctrl+⌫ in terminals without the kitty protocol: ^H
        'w' | 'h' => area.backspace(true),
        // ⌘⌫ in macOS terminals (ctrl+u, readline): delete to the start of the row; ctrl+k: to its end
        'u' => area.kill_to_edge(false),
        'k' => area.kill_to_edge(true),
        _ => return Outcome::Ignored,
    }

    Outcome::Handled
}

fn edit_key(key: KeyEvent, area: &mut TextArea) -> Outcome {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);

    // ⌘ (kitty protocol: super) = ctrl for the edit keys (undo, redo, all, copy, cut, paste)
    let sup = key.modifiers.contains(KeyModifiers::SUPER);
    let page = (area.area.height.max(2) - 1) as isize;

    if !matches!(
        key.code,
        KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown
    ) {
        area.want_x = None;
    }

    match key.code {
        KeyCode::Char(character) if ctrl || sup => return command_key(area, key, character),
        // macOS terminals send option+←/→ as ESC b / ESC f (meta), option+⌫ as ESC DEL, option+fn+⌫ as ESC d
        KeyCode::Char('b') if alt => area.move_to(area.word_left(area.cur), shift),
        KeyCode::Char('f') if alt => area.move_to(area.word_right(area.cur), shift),
        KeyCode::Char('d') if alt => area.delete(true),
        KeyCode::Char('B') if alt => area.move_to(area.word_left(area.cur), true),
        KeyCode::Char('F') if alt => area.move_to(area.word_right(area.cur), true),
        KeyCode::Char(' ') if !alt && area.indent_item() => {}
        KeyCode::Char(character_value) if !alt => area.insert(&character_value.to_string()),
        KeyCode::Enter if !area.single && !ctrl && !alt => area.newline(),
        KeyCode::Backspace | KeyCode::Delete | KeyCode::Left | KeyCode::Right => horizontal_key(key, area),
        KeyCode::Up if !area.single => area.vertical(-1, shift),
        KeyCode::Down if !area.single => area.vertical(1, shift),
        KeyCode::PageUp if !area.single => area.vertical(-page, shift),
        KeyCode::PageDown if !area.single => area.vertical(page, shift),
        KeyCode::Home if ctrl => area.move_to(Pos::default(), shift),
        KeyCode::End if ctrl => {
            let result = area.lines.len() - 1;

            area.move_to(
                Pos {
                    row: result,
                    col: area.lines[result].len(),
                },
                shift,
            );
        }
        KeyCode::Home => area.line_edge(false, shift),
        KeyCode::End => area.line_edge(true, shift),
        _ => return Outcome::Ignored,
    }

    Outcome::Handled
}

fn horizontal_key(key: KeyEvent, area: &mut TextArea) {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);

    match key.code {
        // the row: ⌘ (kitty protocol: super), alt off macOS. ⌫: from the cursor to the row's
        // start, ←→: to the row's start / end
        KeyCode::Backspace if line_mod(key.modifiers) => area.kill_to_edge(false),
        KeyCode::Delete if line_mod(key.modifiers) => area.kill_to_edge(true),
        KeyCode::Left if line_mod(key.modifiers) => area.line_edge(false, shift),
        KeyCode::Right if line_mod(key.modifiers) => area.line_edge(true, shift),
        // the string: ctrl, ⌥ (= alt on macOS). ⌫: the string left of the cursor (in a path:
        // 1 segment), ←→: word jumps
        KeyCode::Backspace => area.backspace(ctrl || alt),
        KeyCode::Delete => area.delete(ctrl || alt),
        KeyCode::Left if ctrl || alt => area.move_to(area.word_left(area.cur), shift),
        KeyCode::Right if ctrl || alt => area.move_to(area.word_right(area.cur), shift),
        KeyCode::Left => match (shift, area.range()) {
            (false, Some((agent, _))) => area.move_to(agent, false),
            _ => area.move_to(area.left_of(area.cur), shift),
        },
        KeyCode::Right => match (shift, area.range()) {
            (false, Some((_, bytes))) => area.move_to(bytes, false),
            _ => area.move_to(area.right_of(area.cur), shift),
        },

        _ => {}
    }
}
